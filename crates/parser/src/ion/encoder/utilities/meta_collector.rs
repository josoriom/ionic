use crate::ion::utilities::EmitAttributes;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use zstd::bulk::compress as zstd_compress;

use crate::{
    accessions::{
        FLOAT_32BIT, FLOAT_64BIT, INTENSITY_ARRAY, MZ_ARRAY, TIME_ARRAY, format_accession,
    },
    decoder::decode::MetadatumValue,
    encoder::utilities::le_writers::{write_f64_slice_le, write_u32_slice_le},
    ion::{
        IonResult,
        attr_meta::{
            ACC_ATTR_COUNT, ACC_ATTR_CV_FULL_NAME, ACC_ATTR_CV_URI, ACC_ATTR_CV_VERSION,
            ACC_ATTR_DEFAULT_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_DEFAULT_SOURCE_FILE_REF,
            ACC_ATTR_ID, ACC_ATTR_INDEX, ACC_ATTR_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_LABEL,
            ACC_ATTR_LOCATION, ACC_ATTR_NAME, ACC_ATTR_ORDER, ACC_ATTR_REF, ACC_ATTR_SAMPLE_REF,
            ACC_ATTR_SCAN_SETTINGS_REF, ACC_ATTR_SOFTWARE_REF, ACC_ATTR_START_TIME_STAMP,
            ACC_ATTR_VERSION, AccessionTail, CV_REF_ATTR, attr_cv_param, cv_ref_code_from_str,
            parse_accession_tail,
        },
        utilities::assign_attributes_into,
    },
    mzml::{
        schema::TagId,
        structs::{
            BinaryDataArray, BinaryDataArrayList, Chromatogram, CvParam, MzML, Precursor,
            PrecursorList, Product, ProductList, ReferenceableParamGroupRef, ScanList, Spectrum,
            SpectrumDescription, UserParam,
        },
    },
};

use super::encoder_output::SectionChunk;
use crate::ion::meta_groups::{META_GROUP_ENTRY_SIZE, MetaGroupEntry, write_group_header};

const USER_PARAM_NAME_VALUE_SEPARATOR: char = '\0';
pub(crate) const LOCAL_LIST_NODE_ID: u32 = 1;
pub(crate) const FIRST_LOCAL_ITEM_NODE_ID: u32 = 2;

pub(crate) struct MetaCollector {
    context: TraversalContext,
}

impl MetaCollector {
    pub(crate) fn new() -> Self {
        Self {
            context: TraversalContext::new(),
        }
    }

    pub(crate) fn collect_global_meta(&mut self, mzml: &MzML) -> (PackedMeta, GlobalCounts) {
        pack_global_meta(mzml, &mut self.context)
    }

    pub(crate) fn add_item<T, L>(
        &mut self,
        item: &T,
        item_index: usize,
        list_node_id: u32,
        list_schema: Option<&L>,
        policy: ArrayPolicy,
        metadata_writer: &mut dyn MetadataWriter,
    ) -> IonResult<()>
    where
        T: MzmlListItem,
        L: EmitAttributes,
    {
        if metadata_writer.is_first_item_in_group() {
            self.context.reset_for_item_group();
        }
        let mut buffer = MetaParamBuffer::new();
        {
            let mut writer = buffer.as_writer();
            if metadata_writer.is_first_item_in_group()
                && list_node_id != 0
                && let Some(schema) = list_schema
            {
                writer.touch(T::list_tag(), list_node_id, 0);
                writer.push_schema_attrs(T::list_tag(), list_node_id, 0, schema);
            }
            let item_id = self.context.alloc();
            writer.push_schema_attrs(T::item_tag(), item_id, list_node_id, item);
            if !item.has_explicit_index() {
                writer.push_optional_u32_attr(
                    T::item_tag(),
                    item_id,
                    list_node_id,
                    ACC_ATTR_INDEX,
                    Some(item_index as u32),
                );
            }
            writer.push_ref_group_params(item_id, item.group_refs(), &mut self.context);
            writer.push_cv_and_user_params(
                item_id,
                list_node_id,
                item.cv_params(),
                item.user_params(),
            );
            item.flatten_children(&mut writer, item_id, &mut self.context, policy);
        }
        buffer.normalize_attr_cv_values();
        metadata_writer.write_metadata_item(&buffer)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ArrayPolicy {
    pub(crate) x_array_accession: u32,
    pub(crate) y_array_accession: u32,
    pub(crate) force_f32: bool,
}

impl ArrayPolicy {
    pub(crate) fn is_xy_array(self, accession: u32) -> bool {
        accession == self.x_array_accession || accession == self.y_array_accession
    }
    pub(crate) fn should_force_f32(self, accession: u32) -> bool {
        self.force_f32 && self.is_xy_array(accession)
    }
}

#[derive(Debug)]
pub struct PackedMeta {
    pub index_offsets: Vec<u32>,
    pub ids: Vec<u32>,
    pub parent_indices: Vec<u32>,
    pub tag_ids: Vec<u8>,
    pub ref_codes: Vec<u8>,
    pub accession_numbers: Vec<u32>,
    pub unit_ref_codes: Vec<u8>,
    pub unit_accession_numbers: Vec<u32>,
    pub value_kinds: Vec<u8>,
    pub value_indices: Vec<u32>,
    pub numeric_values: Vec<f64>,
    pub string_offsets: Vec<u32>,
    pub string_lengths: Vec<u32>,
    pub string_bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct GlobalCounts {
    pub(crate) n_file_description: u32,
    pub(crate) n_run: u32,
    pub(crate) n_ref_param_groups: u32,
    pub(crate) n_samples: u32,
    pub(crate) n_instrument_configs: u32,
    pub(crate) n_software: u32,
    pub(crate) n_data_processing: u32,
    pub(crate) n_acquisition_settings: u32,
    pub(crate) n_cvs: u32,
}

pub(crate) struct GroupedSection {
    pub(crate) section: SectionChunk,
    pub(crate) byte_len: u64,
    pub(crate) crc32: u32,
    pub(crate) group_count: u64,
    pub(crate) uncompressed_size: u64,
    pub(crate) row_count: u64,
    pub(crate) numeric_count: u64,
    pub(crate) string_count: u64,
}

pub(crate) struct MetaGrouper {
    group_size: u32,
    level: u8,
    builder: PackedMetaBuilder,
    items_in_group: u32,
    payloads: SectionChunk,
    crc: crc32fast::Hasher,
    directory: Vec<MetaGroupEntry>,
    uncompressed_size: u64,
    group_count: u64,
    row_count: u64,
    numeric_count: u64,
    string_count: u64,
}

impl MetaGrouper {
    pub(crate) fn new(group_size: u32, level: u8, payloads: SectionChunk) -> Self {
        Self {
            group_size,
            level,
            builder: PackedMetaBuilder::new(),
            items_in_group: 0,
            payloads,
            crc: crc32fast::Hasher::new(),
            directory: Vec::new(),
            uncompressed_size: 0,
            group_count: 0,
            row_count: 0,
            numeric_count: 0,
            string_count: 0,
        }
    }

    fn seal_group(&mut self) -> IonResult<()> {
        if self.items_in_group == 0 {
            return Ok(());
        }
        let meta = std::mem::replace(&mut self.builder, PackedMetaBuilder::new()).build();
        self.row_count += meta.ref_codes.len() as u64;
        self.numeric_count += meta.numeric_values.len() as u64;
        self.string_count += meta.string_offsets.len() as u64;
        let raw = serialize_group(&meta, 0, self.items_in_group as usize);
        let raw_size = raw.len() as u64;
        self.uncompressed_size += raw_size;
        let payload_offset = self.payloads.len();
        let compressed = compress_bytes_if_enabled(raw, self.level);
        self.directory.push(MetaGroupEntry {
            payload_offset,
            payload_size: compressed.len() as u64,
            uncompressed_size: raw_size,
            checksum: crc32fast::hash(&compressed),
        });
        self.crc.update(&compressed);
        self.payloads.write(&compressed)?;
        self.group_count += 1;
        self.items_in_group = 0;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> IonResult<GroupedSection> {
        self.seal_group()?;
        let mut directory_bytes = Vec::with_capacity(self.directory.len() * META_GROUP_ENTRY_SIZE);
        for entry in &self.directory {
            entry.write_into(&mut directory_bytes);
        }
        self.crc.update(&directory_bytes);
        self.payloads.write(&directory_bytes)?;
        let byte_len = self.payloads.len();
        let crc32 = self.crc.finalize();
        Ok(GroupedSection {
            section: self.payloads,
            byte_len,
            crc32,
            group_count: self.group_count,
            uncompressed_size: self.uncompressed_size,
            row_count: self.row_count,
            numeric_count: self.numeric_count,
            string_count: self.string_count,
        })
    }
}

impl MetadataWriter for MetaGrouper {
    fn write_metadata_item(&mut self, buffer: &MetaParamBuffer) -> IonResult<()> {
        self.builder.flush_buffer(buffer);
        self.items_in_group += 1;
        if self.items_in_group == self.group_size {
            self.seal_group()?;
        }
        Ok(())
    }
    fn is_first_item_in_group(&self) -> bool {
        self.items_in_group == 0
    }
}

fn serialize_group(meta: &PackedMeta, item_start: usize, item_end: usize) -> Vec<u8> {
    let row_start = meta.index_offsets[item_start] as usize;
    let row_end = meta.index_offsets[item_end] as usize;
    let meta_count = row_end - row_start;
    let first_row = meta.index_offsets[item_start];

    let mut local_index_offsets = Vec::with_capacity(item_end - item_start + 1);
    for item in item_start..=item_end {
        local_index_offsets.push(meta.index_offsets[item] - first_row);
    }

    let mut numeric_values = Vec::new();
    let mut string_offsets = Vec::new();
    let mut string_lengths = Vec::new();
    let mut string_bytes = Vec::new();
    let mut value_indices = Vec::with_capacity(meta_count);
    for row in row_start..row_end {
        match meta.value_kinds[row] {
            0 => {
                let source = meta.value_indices[row] as usize;
                value_indices.push(numeric_values.len() as u32);
                numeric_values.push(meta.numeric_values[source]);
            }
            1 => {
                let source = meta.value_indices[row] as usize;
                let offset = meta.string_offsets[source] as usize;
                let length = meta.string_lengths[source] as usize;
                value_indices.push(string_offsets.len() as u32);
                string_offsets.push(string_bytes.len() as u32);
                string_lengths.push(length as u32);
                string_bytes.extend_from_slice(&meta.string_bytes[offset..offset + length]);
            }
            _ => {
                value_indices.push(0);
            }
        }
    }

    let mut out = Vec::new();
    write_group_header(
        &mut out,
        meta_count as u32,
        numeric_values.len() as u32,
        string_offsets.len() as u32,
    );
    write_u32_slice_le(&mut out, &local_index_offsets);
    write_u32_slice_le(&mut out, &meta.ids[row_start..row_end]);
    write_u32_slice_le(&mut out, &meta.parent_indices[row_start..row_end]);
    out.extend_from_slice(&meta.tag_ids[row_start..row_end]);
    out.extend_from_slice(&meta.ref_codes[row_start..row_end]);
    write_u32_slice_le(&mut out, &meta.accession_numbers[row_start..row_end]);
    out.extend_from_slice(&meta.unit_ref_codes[row_start..row_end]);
    write_u32_slice_le(&mut out, &meta.unit_accession_numbers[row_start..row_end]);
    out.extend_from_slice(&meta.value_kinds[row_start..row_end]);
    write_u32_slice_le(&mut out, &value_indices);
    write_f64_slice_le(&mut out, &numeric_values);
    write_u32_slice_le(&mut out, &string_offsets);
    write_u32_slice_le(&mut out, &string_lengths);
    out.extend_from_slice(&string_bytes);
    out
}

struct IdAllocator(u32);

impl IdAllocator {
    fn new() -> Self {
        Self(1)
    }
    #[inline]
    fn next(&mut self) -> u32 {
        let id = self.0;
        self.0 += 1;
        id
    }
    fn reset_to(&mut self, next: u32) {
        self.0 = next;
    }
}

pub(crate) struct TraversalContext {
    nodes: IdAllocator,
}

impl TraversalContext {
    fn new() -> Self {
        Self {
            nodes: IdAllocator::new(),
        }
    }
    #[inline]
    fn alloc(&mut self) -> u32 {
        self.nodes.next()
    }
    fn reset_for_item_group(&mut self) {
        self.nodes.reset_to(FIRST_LOCAL_ITEM_NODE_ID);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ValueEncoding {
    kind: u8,
    index: u32,
}

struct ValuePool {
    numeric_values: Vec<f64>,
    string_offsets: Vec<u32>,
    string_lengths: Vec<u32>,
    string_bytes: Vec<u8>,
    numeric_count: u32,
    string_count: u32,
}

impl ValuePool {
    fn new() -> Self {
        Self {
            numeric_values: Vec::new(),
            string_offsets: Vec::new(),
            string_lengths: Vec::new(),
            string_bytes: Vec::new(),
            numeric_count: 0,
            string_count: 0,
        }
    }

    fn encode(&mut self, value: Option<&str>) -> ValueEncoding {
        match value {
            None | Some("") => ValueEncoding { kind: 2, index: 0 },
            Some(text) => {
                let looks_numeric = text.contains('.')
                    || text.contains('e')
                    || text.contains('E')
                    || text.starts_with('-') && text[1..].contains('.');
                if looks_numeric && let Ok(n) = text.parse::<f64>() {
                    let index = self.numeric_count;
                    self.numeric_values.push(n);
                    self.numeric_count += 1;
                    return ValueEncoding { kind: 0, index };
                }
                let index = self.string_count;
                let bytes = text.as_bytes();
                self.string_offsets.push(self.string_bytes.len() as u32);
                self.string_lengths.push(bytes.len() as u32);
                self.string_bytes.extend_from_slice(bytes);
                self.string_count += 1;
                ValueEncoding { kind: 1, index }
            }
        }
    }

    fn encode_as_string(&mut self, value: Option<&str>) -> ValueEncoding {
        match value {
            None | Some("") => ValueEncoding { kind: 2, index: 0 },
            Some(text) => {
                let index = self.string_count;
                let bytes = text.as_bytes();
                self.string_offsets.push(self.string_bytes.len() as u32);
                self.string_lengths.push(bytes.len() as u32);
                self.string_bytes.extend_from_slice(bytes);
                self.string_count += 1;
                ValueEncoding { kind: 1, index }
            }
        }
    }
}

struct MetaParamRow {
    cv_param: CvParam,
    tag_id: u8,
    id: u32,
    parent_id: u32,
}

pub(crate) struct MetaParamBuffer {
    rows: Vec<MetaParamRow>,
}

impl MetaParamBuffer {
    fn new() -> Self {
        Self {
            rows: Vec::with_capacity(64),
        }
    }

    fn push(&mut self, tag: TagId, id: u32, parent_id: u32, cv_param: CvParam) {
        self.rows.push(MetaParamRow {
            cv_param,
            tag_id: tag as u8,
            id,
            parent_id,
        });
    }

    fn extend_cv_params(&mut self, tag: TagId, id: u32, parent_id: u32, cv_params: &[CvParam]) {
        self.rows.reserve(cv_params.len());
        for cv_param in cv_params {
            self.rows.push(MetaParamRow {
                cv_param: cv_param.clone(),
                tag_id: tag as u8,
                id,
                parent_id,
            });
        }
    }

    fn extend_user_params(
        &mut self,
        tag: TagId,
        id: u32,
        parent_id: u32,
        user_params: &[UserParam],
    ) {
        self.rows.reserve(user_params.len());
        for user_param in user_params {
            self.rows.push(MetaParamRow {
                cv_param: encode_user_param_as_cv(user_param),
                tag_id: tag as u8,
                id,
                parent_id,
            });
        }
    }

    fn as_writer(&mut self) -> MetaParamWriter<'_> {
        MetaParamWriter { buffer: self }
    }

    fn normalize_attr_cv_values(&mut self) {
        for row in &mut self.rows {
            if row.cv_param.cv_ref.as_deref() == Some(CV_REF_ATTR) {
                let absent = row.cv_param.value.as_deref().is_none_or(str::is_empty);
                if absent && !row.cv_param.name.is_empty() {
                    row.cv_param.value = Some(std::mem::take(&mut row.cv_param.name));
                }
            }
        }
    }
}

pub(crate) struct MetaParamWriter<'b> {
    buffer: &'b mut MetaParamBuffer,
}

impl<'b> MetaParamWriter<'b> {
    fn push_one(&mut self, tag: TagId, id: u32, parent_id: u32, cv_param: CvParam) {
        self.buffer.push(tag, id, parent_id, cv_param);
    }
    fn push_many(&mut self, tag: TagId, id: u32, parent_id: u32, cv_params: &[CvParam]) {
        self.buffer.extend_cv_params(tag, id, parent_id, cv_params);
    }
    fn push_user_params(&mut self, tag: TagId, id: u32, parent_id: u32, user_params: &[UserParam]) {
        self.buffer
            .extend_user_params(tag, id, parent_id, user_params);
    }
    fn touch(&mut self, tag: TagId, id: u32, parent_id: u32) {
        self.push_one(tag, id, parent_id, empty_cv_param());
    }
    fn push_str_attr(
        &mut self,
        tag: TagId,
        id: u32,
        parent_id: u32,
        tail: AccessionTail,
        value: &str,
    ) {
        if !value.is_empty() {
            self.push_one(tag, id, parent_id, attr_cv_param(tail, value));
        }
    }
    fn push_optional_u32_attr(
        &mut self,
        tag: TagId,
        id: u32,
        parent_id: u32,
        tail: AccessionTail,
        value: Option<u32>,
    ) {
        if let Some(n) = value {
            self.push_one(tag, id, parent_id, attr_cv_param(tail, &n.to_string()));
        }
    }
    fn push_cv_and_user_params(
        &mut self,
        id: u32,
        parent_id: u32,
        cv_params: &[CvParam],
        user_params: &[UserParam],
    ) {
        self.push_many(TagId::CvParam, id, parent_id, cv_params);
        self.push_user_params(TagId::UserParam, id, parent_id, user_params);
    }

    fn push_ref_group_params(
        &mut self,
        id: u32,
        group_refs: &[ReferenceableParamGroupRef],
        context: &mut TraversalContext,
    ) {
        for gr in group_refs {
            let ref_id = context.alloc();
            self.touch(TagId::ReferenceableParamGroupRef, ref_id, id);
            self.push_str_attr(
                TagId::ReferenceableParamGroupRef,
                ref_id,
                id,
                ACC_ATTR_REF,
                &gr.r#ref,
            );
        }
    }
    fn push_schema_attrs<T: EmitAttributes>(
        &mut self,
        tag: TagId,
        id: u32,
        parent_id: u32,
        schema_value: &T,
    ) {
        let mut attrs = Vec::new();
        assign_attributes_into(schema_value, tag, id, parent_id, &mut attrs);
        for attr in attrs {
            let tail_raw = parse_accession_tail_raw(attr.accession.as_deref());
            if tail_raw == 0 {
                continue;
            }
            let text = match attr.value {
                MetadatumValue::Text(t) => t,
                MetadatumValue::Number(n) => n.to_string(),
                MetadatumValue::Empty => continue,
            };
            if text.is_empty() {
                continue;
            }
            self.push_one(
                tag,
                id,
                parent_id,
                attr_cv_param(AccessionTail::from_raw(tail_raw), &text),
            );
        }
    }
}

struct PackedMetaBuilder {
    index_offsets: Vec<u32>,
    ids: Vec<u32>,
    parent_indices: Vec<u32>,
    tag_ids: Vec<u8>,
    ref_codes: Vec<u8>,
    accession_numbers: Vec<u32>,
    unit_ref_codes: Vec<u8>,
    unit_accession_numbers: Vec<u32>,
    value_kinds: Vec<u8>,
    value_indices: Vec<u32>,
    value_pool: ValuePool,
    row_count: u32,
}

impl PackedMetaBuilder {
    fn new() -> Self {
        let mut b = Self {
            index_offsets: Vec::new(),
            ids: Vec::new(),
            parent_indices: Vec::new(),
            tag_ids: Vec::new(),
            ref_codes: Vec::new(),
            accession_numbers: Vec::new(),
            unit_ref_codes: Vec::new(),
            unit_accession_numbers: Vec::new(),
            value_kinds: Vec::new(),
            value_indices: Vec::new(),
            value_pool: ValuePool::new(),
            row_count: 0,
        };
        b.index_offsets.push(0);
        b
    }

    fn flush_buffer(&mut self, buffer: &MetaParamBuffer) {
        for row in &buffer.rows {
            self.push_row(row.tag_id, row.id, row.parent_id, &row.cv_param);
        }
        self.end_item();
    }

    fn push_row(&mut self, tag_id: u8, id: u32, parent_id: u32, cv_param: &CvParam) {
        self.tag_ids.push(tag_id);
        self.ids.push(id);
        self.parent_indices.push(parent_id);

        let cv_ref = cv_ref_prefix_from_accession(cv_param.accession.as_deref())
            .or(cv_param.cv_ref.as_deref());
        self.ref_codes.push(cv_ref_code_from_str(cv_ref));
        self.accession_numbers
            .push(parse_accession_tail_raw(cv_param.accession.as_deref()));

        let unit_ref = cv_ref_prefix_from_accession(cv_param.unit_accession.as_deref())
            .or(cv_param.unit_cv_ref.as_deref());
        self.unit_ref_codes.push(cv_ref_code_from_str(unit_ref));
        self.unit_accession_numbers
            .push(parse_accession_tail_raw(cv_param.unit_accession.as_deref()));

        let is_attr = cv_param.cv_ref.as_deref() == Some(CV_REF_ATTR)
            || cv_ref_prefix_from_accession(cv_param.accession.as_deref()) == Some(CV_REF_ATTR);

        let enc = if is_attr {
            self.value_pool.encode_as_string(cv_param.value.as_deref())
        } else {
            self.value_pool.encode(cv_param.value.as_deref())
        };

        self.value_kinds.push(enc.kind);
        self.value_indices.push(enc.index);
        self.row_count += 1;
    }

    fn end_item(&mut self) {
        self.index_offsets.push(self.row_count);
    }

    fn build(self) -> PackedMeta {
        PackedMeta {
            index_offsets: self.index_offsets,
            ids: self.ids,
            parent_indices: self.parent_indices,
            tag_ids: self.tag_ids,
            ref_codes: self.ref_codes,
            accession_numbers: self.accession_numbers,
            unit_ref_codes: self.unit_ref_codes,
            unit_accession_numbers: self.unit_accession_numbers,
            value_kinds: self.value_kinds,
            value_indices: self.value_indices,
            numeric_values: self.value_pool.numeric_values,
            string_offsets: self.value_pool.string_offsets,
            string_lengths: self.value_pool.string_lengths,
            string_bytes: self.value_pool.string_bytes,
        }
    }
}

fn empty_cv_param() -> CvParam {
    CvParam {
        cv_ref: None,
        accession: None,
        name: String::new(),
        value: None,
        unit_cv_ref: None,
        unit_name: None,
        unit_accession: None,
    }
}

fn encode_user_param_as_cv(p: &UserParam) -> CvParam {
    let value = p.value.as_deref().unwrap_or("");
    let type_ = p.r#type.as_deref().unwrap_or("");
    let encoded = format!(
        "{}{}{}{}{}",
        p.name, USER_PARAM_NAME_VALUE_SEPARATOR, value, USER_PARAM_NAME_VALUE_SEPARATOR, type_
    );
    CvParam {
        cv_ref: None,
        accession: None,
        name: String::new(),
        value: Some(encoded),
        unit_cv_ref: p.unit_cv_ref.clone(),
        unit_name: p.unit_name.clone(),
        unit_accession: p.unit_accession.clone(),
    }
}

fn make_float_precision_cv_param(accession_tail: u32) -> CvParam {
    let name = if accession_tail == FLOAT_32BIT {
        "32-bit float"
    } else {
        "64-bit float"
    };
    CvParam {
        cv_ref: Some("MS".to_string()),
        accession: Some(format_accession(accession_tail)),
        name: name.to_string(),
        value: None,
        unit_cv_ref: None,
        unit_name: None,
        unit_accession: None,
    }
}

#[inline]
pub(crate) fn parse_accession_tail_raw(accession: Option<&str>) -> u32 {
    parse_accession_tail(accession).raw()
}

fn cv_ref_prefix_from_accession(accession: Option<&str>) -> Option<&str> {
    accession.and_then(|s| s.split_once(':').map(|(prefix, _)| prefix))
}

pub(crate) fn array_type_accession_from_binary_data_array(bda: &BinaryDataArray) -> u32 {
    for cv in &bda.cv_params {
        let t = parse_accession_tail_raw(cv.accession.as_deref());
        if matches!(t, MZ_ARRAY | INTENSITY_ARRAY | TIME_ARRAY) {
            return t;
        }
    }
    for cv in &bda.cv_params {
        let t = parse_accession_tail_raw(cv.accession.as_deref());
        if t != 0 && cv.name.to_ascii_lowercase().contains(" array") {
            return t;
        }
    }
    0
}

fn emit_binary_data_array_cv_params(
    writer: &mut MetaParamWriter<'_>,
    bda_node_id: u32,
    bda_list_node_id: u32,
    bda: &BinaryDataArray,
    policy: ArrayPolicy,
) {
    let array_acc = array_type_accession_from_binary_data_array(bda);
    if !(policy.force_f32 && policy.is_xy_array(array_acc)) {
        writer.push_many(
            TagId::CvParam,
            bda_node_id,
            bda_list_node_id,
            &bda.cv_params,
        );
        return;
    }
    let mut precision_written = false;
    for cv in &bda.cv_params {
        let tail = parse_accession_tail_raw(cv.accession.as_deref());
        if tail == FLOAT_32BIT || tail == FLOAT_64BIT {
            if !precision_written {
                writer.push_one(
                    TagId::CvParam,
                    bda_node_id,
                    bda_list_node_id,
                    make_float_precision_cv_param(FLOAT_32BIT),
                );
                precision_written = true;
            }
        } else {
            writer.push_one(TagId::CvParam, bda_node_id, bda_list_node_id, cv.clone());
        }
    }
    if !precision_written {
        writer.push_one(
            TagId::CvParam,
            bda_node_id,
            bda_list_node_id,
            make_float_precision_cv_param(FLOAT_32BIT),
        );
    }
}

fn packed_meta_byte_size(m: &PackedMeta) -> usize {
    m.index_offsets.len() * 4
        + m.ids.len() * 4
        + m.parent_indices.len() * 4
        + m.tag_ids.len()
        + m.ref_codes.len()
        + m.accession_numbers.len() * 4
        + m.unit_ref_codes.len()
        + m.unit_accession_numbers.len() * 4
        + m.value_kinds.len()
        + m.value_indices.len() * 4
        + m.numeric_values.len() * 8
        + m.string_offsets.len() * 4
        + m.string_lengths.len() * 4
        + m.string_bytes.len()
}

fn write_packed_meta(buf: &mut Vec<u8>, m: &PackedMeta) {
    write_u32_slice_le(buf, &m.index_offsets);
    write_u32_slice_le(buf, &m.ids);
    write_u32_slice_le(buf, &m.parent_indices);
    buf.extend_from_slice(&m.tag_ids);
    buf.extend_from_slice(&m.ref_codes);
    write_u32_slice_le(buf, &m.accession_numbers);
    buf.extend_from_slice(&m.unit_ref_codes);
    write_u32_slice_le(buf, &m.unit_accession_numbers);
    buf.extend_from_slice(&m.value_kinds);
    write_u32_slice_le(buf, &m.value_indices);
    write_f64_slice_le(buf, &m.numeric_values);
    write_u32_slice_le(buf, &m.string_offsets);
    write_u32_slice_le(buf, &m.string_lengths);
    buf.extend_from_slice(&m.string_bytes);
}

pub(crate) fn serialize_global_meta_with_counts(counts: &GlobalCounts, m: &PackedMeta) -> Vec<u8> {
    let mut buf = Vec::with_capacity(32 + packed_meta_byte_size(m));
    for n in [
        counts.n_file_description as u16,
        counts.n_ref_param_groups as u16,
        counts.n_samples as u16,
        counts.n_instrument_configs as u16,
        counts.n_software as u16,
        counts.n_data_processing as u16,
        counts.n_acquisition_settings as u16,
        counts.n_cvs as u16,
        counts.n_run as u16,
    ] {
        buf.extend_from_slice(&n.to_le_bytes());
    }
    buf.extend_from_slice(&[0u8; 14]);
    write_packed_meta(&mut buf, m);
    buf
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub(crate) fn compress_bytes_if_enabled(bytes: Vec<u8>, level: u8) -> Vec<u8> {
    if level == 0 {
        bytes
    } else {
        zstd_compress(&bytes, level as i32).expect("zstd compression failed")
    }
}

#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
pub(crate) fn compress_bytes_if_enabled(bytes: Vec<u8>, level: u8) -> Vec<u8> {
    if level == 0 {
        bytes
    } else {
        panic!("zstd compression is not available in browser wasm")
    }
}

pub(crate) trait MetadataWriter {
    fn write_metadata_item(&mut self, buffer: &MetaParamBuffer) -> IonResult<()>;
    fn is_first_item_in_group(&self) -> bool {
        false
    }
}

impl MetadataWriter for PackedMetaBuilder {
    fn write_metadata_item(&mut self, buffer: &MetaParamBuffer) -> IonResult<()> {
        self.flush_buffer(buffer);
        Ok(())
    }
}

fn append_meta_buffer(
    buffers: &mut Vec<MetaParamBuffer>,
    fill: impl FnOnce(&mut MetaParamWriter<'_>),
) {
    let mut buffer = MetaParamBuffer::new();
    fill(&mut buffer.as_writer());
    buffer.normalize_attr_cv_values();
    buffers.push(buffer);
}

pub(crate) trait MzmlListItem: EmitAttributes {
    fn list_tag() -> TagId;
    fn item_tag() -> TagId;
    fn has_explicit_index(&self) -> bool;
    fn cv_params(&self) -> &[CvParam];
    fn user_params(&self) -> &[UserParam];
    fn group_refs(&self) -> &[ReferenceableParamGroupRef];
    fn flatten_children(
        &self,
        writer: &mut MetaParamWriter<'_>,
        item_id: u32,
        context: &mut TraversalContext,
        policy: ArrayPolicy,
    );
}

impl MzmlListItem for Spectrum {
    fn list_tag() -> TagId {
        TagId::SpectrumList
    }
    fn item_tag() -> TagId {
        TagId::Spectrum
    }
    fn has_explicit_index(&self) -> bool {
        self.index.is_some()
    }
    fn cv_params(&self) -> &[CvParam] {
        &self.cv_params
    }
    fn user_params(&self) -> &[UserParam] {
        &self.user_params
    }
    fn group_refs(&self) -> &[ReferenceableParamGroupRef] {
        &self.referenceable_param_group_refs
    }
    fn flatten_children(
        &self,
        writer: &mut MetaParamWriter<'_>,
        id: u32,
        context: &mut TraversalContext,
        policy: ArrayPolicy,
    ) {
        flatten_spectrum_children(writer, self, id, context, policy);
    }
}

impl MzmlListItem for Chromatogram {
    fn list_tag() -> TagId {
        TagId::ChromatogramList
    }
    fn item_tag() -> TagId {
        TagId::Chromatogram
    }
    fn has_explicit_index(&self) -> bool {
        true
    }
    fn cv_params(&self) -> &[CvParam] {
        &self.cv_params
    }
    fn user_params(&self) -> &[UserParam] {
        &self.user_params
    }
    fn group_refs(&self) -> &[ReferenceableParamGroupRef] {
        &self.referenceable_param_group_refs
    }
    fn flatten_children(
        &self,
        writer: &mut MetaParamWriter<'_>,
        id: u32,
        context: &mut TraversalContext,
        policy: ArrayPolicy,
    ) {
        flatten_chromatogram_children(writer, self, id, context, policy);
    }
}

fn flatten_spectrum_children(
    writer: &mut MetaParamWriter<'_>,
    spectrum: &Spectrum,
    spectrum_id: u32,
    context: &mut TraversalContext,
    policy: ArrayPolicy,
) {
    if let Some(desc) = &spectrum.spectrum_description {
        flatten_legacy_spectrum_description(writer, desc, spectrum_id, context);
    }
    flatten_scan_list_opt(writer, spectrum.scan_list.as_ref(), spectrum_id, context);
    flatten_precursor_list_opt(
        writer,
        spectrum.precursor_list.as_ref(),
        spectrum_id,
        context,
    );
    flatten_product_list_opt(writer, spectrum.product_list.as_ref(), spectrum_id, context);
    flatten_binary_data_array_list(
        writer,
        spectrum.binary_data_array_list.as_ref(),
        spectrum_id,
        context,
        policy,
    );
}

fn flatten_legacy_spectrum_description(
    writer: &mut MetaParamWriter<'_>,
    desc: &SpectrumDescription,
    spectrum_id: u32,
    context: &mut TraversalContext,
) {
    let desc_id = context.alloc();
    writer.touch(TagId::SpectrumDescription, desc_id, spectrum_id);
    writer.push_ref_group_params(desc_id, &desc.referenceable_param_group_refs, context);
    writer.push_cv_and_user_params(desc_id, spectrum_id, &desc.cv_params, &desc.user_params);
    flatten_scan_list_opt(writer, desc.scan_list.as_ref(), desc_id, context);
    flatten_precursor_list_opt(writer, desc.precursor_list.as_ref(), desc_id, context);
    flatten_product_list_opt(writer, desc.product_list.as_ref(), desc_id, context);
}

fn flatten_chromatogram_children(
    writer: &mut MetaParamWriter<'_>,
    chrom: &Chromatogram,
    chrom_id: u32,
    context: &mut TraversalContext,
    policy: ArrayPolicy,
) {
    if let Some(p) = &chrom.precursor {
        flatten_precursor(writer, p, chrom_id, context);
    }
    if let Some(p) = &chrom.product {
        flatten_product(writer, p, chrom_id, context);
    }
    flatten_binary_data_array_list(
        writer,
        chrom.binary_data_array_list.as_ref(),
        chrom_id,
        context,
        policy,
    );
}

fn flatten_scan_list_opt(
    writer: &mut MetaParamWriter<'_>,
    sl: Option<&ScanList>,
    parent: u32,
    context: &mut TraversalContext,
) {
    let Some(sl) = sl else { return };
    let sl_id = context.alloc();
    writer.push_optional_u32_attr(
        TagId::ScanList,
        sl_id,
        parent,
        ACC_ATTR_COUNT,
        Some(sl.scans.len() as u32),
    );
    writer.push_ref_group_params(sl_id, &sl.referenceable_param_group_refs, context);
    writer.push_cv_and_user_params(sl_id, parent, &sl.cv_params, &sl.user_params);
    flatten_scan_list(writer, sl, sl_id, context);
}

fn flatten_precursor_list_opt(
    writer: &mut MetaParamWriter<'_>,
    pl: Option<&PrecursorList>,
    parent: u32,
    context: &mut TraversalContext,
) {
    let Some(pl) = pl else { return };
    let pl_id = context.alloc();
    writer.push_optional_u32_attr(
        TagId::PrecursorList,
        pl_id,
        parent,
        ACC_ATTR_COUNT,
        Some(pl.precursors.len() as u32),
    );
    writer.push_cv_and_user_params(pl_id, parent, &pl.cv_params, &pl.user_params);
    for p in &pl.precursors {
        flatten_precursor(writer, p, pl_id, context);
    }
}

fn flatten_product_list_opt(
    writer: &mut MetaParamWriter<'_>,
    pl: Option<&ProductList>,
    parent: u32,
    context: &mut TraversalContext,
) {
    let Some(pl) = pl else { return };
    let pl_id = context.alloc();
    writer.push_optional_u32_attr(
        TagId::ProductList,
        pl_id,
        parent,
        ACC_ATTR_COUNT,
        Some(pl.products.len() as u32),
    );
    writer.push_cv_and_user_params(pl_id, parent, &pl.cv_params, &pl.user_params);
    for p in &pl.products {
        flatten_product(writer, p, pl_id, context);
    }
}

fn flatten_binary_data_array_list(
    writer: &mut MetaParamWriter<'_>,
    bda_list: Option<&BinaryDataArrayList>,
    parent_id: u32,
    context: &mut TraversalContext,
    policy: ArrayPolicy,
) {
    let Some(list) = bda_list else { return };
    let list_id = context.alloc();
    writer.push_optional_u32_attr(
        TagId::BinaryDataArrayList,
        list_id,
        parent_id,
        ACC_ATTR_COUNT,
        Some(list.binary_data_arrays.len() as u32),
    );
    for bda in &list.binary_data_arrays {
        let bda_id = context.alloc();
        writer.touch(TagId::BinaryDataArray, bda_id, list_id);
        writer.push_schema_attrs(TagId::BinaryDataArray, bda_id, list_id, bda);
        writer.push_ref_group_params(bda_id, &bda.referenceable_param_group_refs, context);
        emit_binary_data_array_cv_params(writer, bda_id, list_id, bda, policy);
    }
}

fn flatten_precursor(
    writer: &mut MetaParamWriter<'_>,
    precursor: &Precursor,
    parent_id: u32,
    context: &mut TraversalContext,
) {
    let p_id = context.alloc();
    writer.touch(TagId::Precursor, p_id, parent_id);
    writer.push_schema_attrs(TagId::Precursor, p_id, parent_id, precursor);
    if let Some(iw) = &precursor.isolation_window {
        let iw_id = context.alloc();
        writer.touch(TagId::IsolationWindow, iw_id, p_id);
        writer.push_ref_group_params(iw_id, &iw.referenceable_param_group_refs, context);
        writer.push_cv_and_user_params(iw_id, p_id, &iw.cv_params, &iw.user_params);
    }
    if let Some(sil) = &precursor.selected_ion_list {
        let sil_id = context.alloc();
        writer.push_optional_u32_attr(
            TagId::SelectedIonList,
            sil_id,
            p_id,
            ACC_ATTR_COUNT,
            Some(sil.selected_ions.len() as u32),
        );
        for si in &sil.selected_ions {
            let si_id = context.alloc();
            writer.touch(TagId::SelectedIon, si_id, sil_id);
            writer.push_ref_group_params(si_id, &si.referenceable_param_group_refs, context);
            writer.push_cv_and_user_params(si_id, sil_id, &si.cv_params, &si.user_params);
        }
    }
    if let Some(act) = &precursor.activation {
        let act_id = context.alloc();
        writer.touch(TagId::Activation, act_id, p_id);
        writer.push_ref_group_params(act_id, &act.referenceable_param_group_refs, context);
        writer.push_cv_and_user_params(act_id, p_id, &act.cv_params, &act.user_params);
    }
}

fn flatten_product(
    writer: &mut MetaParamWriter<'_>,
    product: &Product,
    parent_id: u32,
    context: &mut TraversalContext,
) {
    let prod_id = context.alloc();
    writer.touch(TagId::Product, prod_id, parent_id);
    writer.push_schema_attrs(TagId::Product, prod_id, parent_id, product);
    writer.push_cv_and_user_params(prod_id, prod_id, &product.cv_params, &product.user_params);
    if let Some(iw) = &product.isolation_window {
        let iw_id = context.alloc();
        writer.touch(TagId::IsolationWindow, iw_id, prod_id);
        writer.push_ref_group_params(iw_id, &iw.referenceable_param_group_refs, context);
        writer.push_cv_and_user_params(iw_id, prod_id, &iw.cv_params, &iw.user_params);
    }
}

fn flatten_scan_list(
    writer: &mut MetaParamWriter<'_>,
    scan_list: &ScanList,
    sl_id: u32,
    context: &mut TraversalContext,
) {
    for scan in &scan_list.scans {
        let scan_id = context.alloc();
        writer.touch(TagId::Scan, scan_id, sl_id);
        writer.push_schema_attrs(TagId::Scan, scan_id, sl_id, scan);
        writer.push_ref_group_params(scan_id, &scan.referenceable_param_group_refs, context);
        writer.push_cv_and_user_params(scan_id, sl_id, &scan.cv_params, &scan.user_params);
        if let Some(swl) = &scan.scan_window_list {
            let swl_id = context.alloc();
            writer.push_optional_u32_attr(
                TagId::ScanWindowList,
                swl_id,
                scan_id,
                ACC_ATTR_COUNT,
                Some(swl.scan_windows.len() as u32),
            );
            for sw in &swl.scan_windows {
                let sw_id = context.alloc();
                writer.touch(TagId::ScanWindow, sw_id, swl_id);
                writer.push_cv_and_user_params(sw_id, swl_id, &sw.cv_params, &sw.user_params);
            }
        }
    }
}

fn pack_global_meta(mzml: &MzML, context: &mut TraversalContext) -> (PackedMeta, GlobalCounts) {
    let mut buffers: Vec<MetaParamBuffer> = Vec::new();

    let n_file_description = append_file_description_meta(mzml, context, &mut buffers);
    let n_run = append_run_meta(mzml, context, &mut buffers);
    let n_ref_param_groups = append_ref_param_groups_meta(mzml, context, &mut buffers);
    let n_samples = append_samples_meta(mzml, context, &mut buffers);
    let n_instrument_configs = append_instruments_meta(mzml, context, &mut buffers);
    let n_software = append_software_list_meta(mzml, context, &mut buffers);
    let n_data_processing = append_data_processing_list_meta(mzml, context, &mut buffers);
    let n_acquisition_settings = append_scan_settings_list_meta(mzml, context, &mut buffers);
    let n_cvs = append_cv_list_meta(mzml, context, &mut buffers);

    let counts = GlobalCounts {
        n_file_description,
        n_run,
        n_ref_param_groups,
        n_samples,
        n_instrument_configs,
        n_software,
        n_data_processing,
        n_acquisition_settings,
        n_cvs,
    };
    let mut builder = PackedMetaBuilder::new();
    for buffer in &buffers {
        builder.flush_buffer(buffer);
    }
    (builder.build(), counts)
}

fn append_file_description_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(fd) = &mzml.file_description else {
        return 0;
    };
    append_meta_buffer(buffers, |writer| {
        let fd_id = context.alloc();
        let fc_id = context.alloc();
        let sfl_id = context.alloc();

        writer.touch(TagId::FileDescription, fd_id, 0);
        writer.touch(TagId::FileContent, fc_id, fd_id);
        writer.push_ref_group_params(
            fc_id,
            &fd.file_content.referenceable_param_group_refs,
            context,
        );
        writer.push_cv_and_user_params(
            fc_id,
            fd_id,
            &fd.file_content.cv_params,
            &fd.file_content.user_params,
        );

        writer.touch(TagId::SourceFileList, sfl_id, fd_id);
        writer.push_optional_u32_attr(
            TagId::SourceFileList,
            sfl_id,
            fd_id,
            ACC_ATTR_COUNT,
            Some(fd.source_file_list.source_file.len() as u32),
        );

        for sf in &fd.source_file_list.source_file {
            let sf_id = context.alloc();
            writer.touch(TagId::SourceFile, sf_id, sfl_id);
            writer.push_str_attr(TagId::SourceFile, sf_id, sfl_id, ACC_ATTR_ID, &sf.id);
            writer.push_str_attr(TagId::SourceFile, sf_id, sfl_id, ACC_ATTR_NAME, &sf.name);
            writer.push_str_attr(
                TagId::SourceFile,
                sf_id,
                sfl_id,
                ACC_ATTR_LOCATION,
                &sf.location,
            );
            writer.push_ref_group_params(sf_id, &sf.referenceable_param_group_ref, context);
            writer.push_cv_and_user_params(sf_id, sfl_id, &sf.cv_param, &sf.user_param);
        }
        for contact in &fd.contacts {
            let c_id = context.alloc();
            writer.touch(TagId::Contact, c_id, fd_id);
            writer.push_ref_group_params(c_id, &contact.referenceable_param_group_refs, context);
            writer.push_cv_and_user_params(c_id, fd_id, &contact.cv_params, &contact.user_params);
        }
    });
    1
}

fn append_run_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let run = &mzml.run;
    append_meta_buffer(buffers, |writer| {
        let run_id = context.alloc();
        writer.touch(TagId::Run, run_id, 0);
        writer.push_str_attr(TagId::Run, run_id, 0, ACC_ATTR_ID, &run.id);
        if let Some(ts) = run.start_time_stamp.as_deref() {
            writer.push_str_attr(TagId::Run, run_id, 0, ACC_ATTR_START_TIME_STAMP, ts);
        }
        if let Some(r) = run.default_instrument_configuration_ref.as_deref() {
            writer.push_str_attr(
                TagId::Run,
                run_id,
                0,
                ACC_ATTR_DEFAULT_INSTRUMENT_CONFIGURATION_REF,
                r,
            );
        }
        if let Some(r) = run.default_source_file_ref.as_deref() {
            writer.push_str_attr(TagId::Run, run_id, 0, ACC_ATTR_DEFAULT_SOURCE_FILE_REF, r);
        }
        if let Some(r) = run.sample_ref.as_deref() {
            writer.push_str_attr(TagId::Run, run_id, 0, ACC_ATTR_SAMPLE_REF, r);
        }
        if let Some(sfrl) = &run.source_file_ref_list {
            let sfrl_id = context.alloc();
            writer.touch(TagId::SourceFileRefList, sfrl_id, run_id);
            writer.push_optional_u32_attr(
                TagId::SourceFileRefList,
                sfrl_id,
                run_id,
                ACC_ATTR_COUNT,
                Some(sfrl.source_file_refs.len() as u32),
            );
            for sfr in &sfrl.source_file_refs {
                let sfr_id = context.alloc();
                writer.touch(TagId::SourceFileRef, sfr_id, sfrl_id);
                writer.push_str_attr(
                    TagId::SourceFileRef,
                    sfr_id,
                    sfrl_id,
                    ACC_ATTR_REF,
                    &sfr.r#ref,
                );
            }
        }
        for gr in &run.referenceable_param_group_refs {
            let gref_id = context.alloc();
            writer.touch(TagId::ReferenceableParamGroupRef, gref_id, run_id);
            writer.push_str_attr(
                TagId::ReferenceableParamGroupRef,
                gref_id,
                run_id,
                ACC_ATTR_REF,
                &gr.r#ref,
            );
        }
        writer.push_cv_and_user_params(run_id, 0, &run.cv_params, &run.user_params);
    });
    1
}

fn append_ref_param_groups_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.referenceable_param_group_list else {
        return 0;
    };
    for group in &list.referenceable_param_groups {
        append_meta_buffer(buffers, |writer| {
            let gid = context.alloc();
            writer.touch(TagId::ReferenceableParamGroup, gid, 0);
            writer.push_str_attr(
                TagId::ReferenceableParamGroup,
                gid,
                0,
                ACC_ATTR_ID,
                &group.id,
            );
            writer.push_cv_and_user_params(gid, 0, &group.cv_params, &group.user_params);
        });
    }
    list.referenceable_param_groups.len() as u32
}

fn append_samples_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.sample_list else {
        return 0;
    };
    let list_id = context.alloc();
    for (i, sample) in list.samples.iter().enumerate() {
        append_meta_buffer(buffers, |writer| {
            if i == 0 {
                writer.touch(TagId::SampleList, list_id, 0);
                writer.push_cv_and_user_params(list_id, 0, &list.cv_params, &list.user_params);
            }
            let sid = context.alloc();
            writer.touch(TagId::Sample, sid, list_id);
            writer.push_str_attr(TagId::Sample, sid, list_id, ACC_ATTR_ID, &sample.id);
            writer.push_str_attr(TagId::Sample, sid, list_id, ACC_ATTR_NAME, &sample.name);
            writer.push_ref_group_params(sid, &sample.referenceable_param_group_refs, context);
            writer.push_cv_and_user_params(sid, list_id, &sample.cv_params, &sample.user_params);
        });
    }
    list.samples.len() as u32
}

#[allow(clippy::too_many_arguments)]
fn emit_instrument_component(
    writer: &mut MetaParamWriter<'_>,
    tag: TagId,
    order: Option<u32>,
    parent_id: u32,
    group_refs: &[ReferenceableParamGroupRef],
    cv_params: &[CvParam],
    user_params: &[UserParam],
    context: &mut TraversalContext,
) {
    let comp_id = context.alloc();
    writer.touch(tag, comp_id, parent_id);
    writer.push_optional_u32_attr(tag, comp_id, parent_id, ACC_ATTR_ORDER, order);
    writer.push_ref_group_params(comp_id, group_refs, context);
    writer.push_cv_and_user_params(comp_id, parent_id, cv_params, user_params);
}

fn append_instruments_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.instrument_list else {
        return 0;
    };
    for inst in &list.instrument {
        append_meta_buffer(buffers, |writer| {
            let id = context.alloc();
            writer.touch(TagId::Instrument, id, 0);
            writer.push_str_attr(TagId::Instrument, id, 0, ACC_ATTR_ID, &inst.id);
            if let Some(ssr) = &inst.scan_settings_ref {
                writer.push_str_attr(
                    TagId::Instrument,
                    id,
                    0,
                    ACC_ATTR_SCAN_SETTINGS_REF,
                    &ssr.r#ref,
                );
            }
            writer.push_ref_group_params(id, &inst.referenceable_param_group_ref, context);
            writer.push_cv_and_user_params(id, 0, &inst.cv_param, &inst.user_param);
            if let Some(sw) = &inst.software_ref {
                let sw_id = context.alloc();
                writer.touch(TagId::SoftwareRef, sw_id, id);
                writer.push_str_attr(TagId::SoftwareRef, sw_id, id, ACC_ATTR_REF, &sw.r#ref);
            }
            if let Some(cl) = &inst.component_list {
                for s in &cl.source {
                    emit_instrument_component(
                        writer,
                        TagId::ComponentSource,
                        s.order,
                        id,
                        &s.referenceable_param_group_ref,
                        &s.cv_param,
                        &s.user_param,
                        context,
                    );
                }
                for a in &cl.analyzer {
                    emit_instrument_component(
                        writer,
                        TagId::ComponentAnalyzer,
                        a.order,
                        id,
                        &a.referenceable_param_group_ref,
                        &a.cv_param,
                        &a.user_param,
                        context,
                    );
                }
                for d in &cl.detector {
                    emit_instrument_component(
                        writer,
                        TagId::ComponentDetector,
                        d.order,
                        id,
                        &d.referenceable_param_group_ref,
                        &d.cv_param,
                        &d.user_param,
                        context,
                    );
                }
            }
        });
    }
    list.instrument.len() as u32
}

fn append_software_list_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.software_list else {
        return 0;
    };
    for sw in &list.software {
        append_meta_buffer(buffers, |writer| {
            let swid = context.alloc();
            writer.touch(TagId::Software, swid, 0);
            writer.push_str_attr(TagId::Software, swid, 0, ACC_ATTR_ID, &sw.id);
            let version = sw
                .version
                .as_deref()
                .or_else(|| sw.software_param.first().and_then(|p| p.version.as_deref()));
            if let Some(v) = version {
                writer.push_str_attr(TagId::Software, swid, 0, ACC_ATTR_VERSION, v);
            }
            writer.push_ref_group_params(swid, &sw.referenceable_param_group_refs, context);
            for sp in &sw.software_param {
                let pid = context.alloc();
                writer.touch(TagId::SoftwareParam, pid, swid);
                writer.push_one(
                    TagId::SoftwareParam,
                    pid,
                    swid,
                    CvParam {
                        cv_ref: sp.cv_ref.clone(),
                        accession: Some(sp.accession.clone()),
                        name: sp.name.clone(),
                        value: Some(String::new()),
                        unit_cv_ref: None,
                        unit_name: None,
                        unit_accession: None,
                    },
                );
            }
            writer.push_cv_and_user_params(swid, 0, &sw.cv_param, &sw.user_params);
        });
    }
    list.software.len() as u32
}

fn append_data_processing_list_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.data_processing_list else {
        return 0;
    };
    for dp in &list.data_processing {
        append_meta_buffer(buffers, |writer| {
            let dp_id = context.alloc();
            writer.touch(TagId::DataProcessing, dp_id, 0);
            writer.push_str_attr(TagId::DataProcessing, dp_id, 0, ACC_ATTR_ID, &dp.id);
            if let Some(sw) = dp.software_ref.as_deref() {
                writer.push_str_attr(TagId::DataProcessing, dp_id, 0, ACC_ATTR_SOFTWARE_REF, sw);
            }
            for pm in &dp.processing_method {
                let pm_id = context.alloc();
                writer.touch(TagId::ProcessingMethod, pm_id, dp_id);
                writer.push_optional_u32_attr(
                    TagId::ProcessingMethod,
                    pm_id,
                    dp_id,
                    ACC_ATTR_ORDER,
                    pm.order,
                );
                if let Some(sw) = pm.software_ref.as_deref() {
                    writer.push_str_attr(
                        TagId::ProcessingMethod,
                        pm_id,
                        dp_id,
                        ACC_ATTR_SOFTWARE_REF,
                        sw,
                    );
                }
                writer.push_ref_group_params(pm_id, &pm.referenceable_param_group_ref, context);
                writer.push_cv_and_user_params(pm_id, dp_id, &pm.cv_param, &pm.user_param);
            }
        });
    }
    list.data_processing.len() as u32
}

fn append_scan_settings_list_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.scan_settings_list else {
        return 0;
    };
    for ss in &list.scan_settings {
        append_meta_buffer(buffers, |writer| {
            let ss_id = context.alloc();
            writer.touch(TagId::ScanSettings, ss_id, 0);
            if let Some(id) = ss.id.as_deref() {
                writer.push_str_attr(TagId::ScanSettings, ss_id, 0, ACC_ATTR_ID, id);
            }
            if let Some(r) = ss.instrument_configuration_ref.as_deref() {
                writer.push_str_attr(
                    TagId::ScanSettings,
                    ss_id,
                    0,
                    ACC_ATTR_INSTRUMENT_CONFIGURATION_REF,
                    r,
                );
            }
            if let Some(sfrl) = &ss.source_file_ref_list {
                let sfrl_id = context.alloc();
                writer.touch(TagId::SourceFileRefList, sfrl_id, ss_id);
                writer.push_optional_u32_attr(
                    TagId::SourceFileRefList,
                    sfrl_id,
                    ss_id,
                    ACC_ATTR_COUNT,
                    Some(sfrl.source_file_refs.len() as u32),
                );
                for sfr in &sfrl.source_file_refs {
                    let sfr_id = context.alloc();
                    writer.touch(TagId::SourceFileRef, sfr_id, sfrl_id);
                    writer.push_str_attr(
                        TagId::SourceFileRef,
                        sfr_id,
                        sfrl_id,
                        ACC_ATTR_REF,
                        &sfr.r#ref,
                    );
                }
            }
            writer.push_ref_group_params(ss_id, &ss.referenceable_param_group_refs, context);
            writer.push_cv_and_user_params(ss_id, 0, &ss.cv_params, &ss.user_params);
            if let Some(tl) = &ss.target_list {
                for target in &tl.targets {
                    let t_id = context.alloc();
                    writer.touch(TagId::Target, t_id, ss_id);
                    writer.push_ref_group_params(
                        t_id,
                        &target.referenceable_param_group_refs,
                        context,
                    );
                    writer.push_cv_and_user_params(
                        t_id,
                        ss_id,
                        &target.cv_params,
                        &target.user_params,
                    );
                }
            }
        });
    }
    list.scan_settings.len() as u32
}

fn append_cv_list_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(cv_list) = &mzml.cv_list else {
        return 0;
    };
    if cv_list.cv.is_empty() {
        return 0;
    }
    let cv_list_id = context.alloc();
    let cv_count = cv_list.cv.len() as u32;
    for (i, cv) in cv_list.cv.iter().enumerate() {
        append_meta_buffer(buffers, |writer| {
            if i == 0 {
                writer.touch(TagId::CvList, cv_list_id, 0);
                writer.push_optional_u32_attr(
                    TagId::CvList,
                    cv_list_id,
                    0,
                    ACC_ATTR_COUNT,
                    Some(cv_count),
                );
            }
            let cv_id = context.alloc();
            writer.touch(TagId::Cv, cv_id, cv_list_id);
            writer.push_str_attr(TagId::Cv, cv_id, cv_list_id, ACC_ATTR_LABEL, &cv.id);
            if let Some(n) = cv.full_name.as_deref().filter(|s| !s.is_empty()) {
                writer.push_str_attr(TagId::Cv, cv_id, cv_list_id, ACC_ATTR_CV_FULL_NAME, n);
            }
            if let Some(v) = cv.version.as_deref().filter(|s| !s.is_empty()) {
                writer.push_str_attr(TagId::Cv, cv_id, cv_list_id, ACC_ATTR_CV_VERSION, v);
            }
            if let Some(u) = cv.uri.as_deref().filter(|s| !s.is_empty()) {
                writer.push_str_attr(TagId::Cv, cv_id, cv_list_id, ACC_ATTR_CV_URI, u);
            }
        });
    }
    cv_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn value_pool_empty_value_gives_kind_2() {
        let mut pool = ValuePool::new();
        assert_eq!(pool.encode(None), ValueEncoding { kind: 2, index: 0 });
        assert_eq!(pool.encode(Some("")), ValueEncoding { kind: 2, index: 0 });
    }

    #[test]
    fn value_pool_numeric_increments_index() {
        let mut pool = ValuePool::new();
        assert_eq!(
            pool.encode(Some("1.5")),
            ValueEncoding { kind: 0, index: 0 }
        );
        assert_eq!(
            pool.encode(Some("2.5")),
            ValueEncoding { kind: 0, index: 1 }
        );
        assert_eq!(pool.numeric_values, vec![1.5f64, 2.5f64]);
    }

    #[test]
    fn value_pool_string_increments_index() {
        let mut pool = ValuePool::new();
        assert_eq!(
            pool.encode(Some("hello")),
            ValueEncoding { kind: 1, index: 0 }
        );
        assert_eq!(
            pool.encode(Some("world")),
            ValueEncoding { kind: 1, index: 1 }
        );
        assert_eq!(&pool.string_bytes[..5], b"hello");
    }

    #[test]
    fn value_pool_string_offsets_are_cumulative() {
        let mut pool = ValuePool::new();
        pool.encode(Some("ab"));
        pool.encode(Some("cde"));
        assert_eq!(pool.string_offsets, vec![0, 2]);
        assert_eq!(pool.string_lengths, vec![2, 3]);
    }

    #[test]
    fn meta_param_buffer_push_records_id() {
        let mut buffer = MetaParamBuffer::new();
        buffer.push(TagId::CvParam, 1, 0, empty_cv_param());
        assert_eq!(buffer.rows.len(), 1);
        assert_eq!(buffer.rows[0].id, 1);
    }

    #[test]
    fn meta_param_buffer_normalize_moves_name_to_value_for_attr_cv() {
        let mut buffer = MetaParamBuffer::new();
        buffer.rows.push(MetaParamRow {
            cv_param: CvParam {
                cv_ref: Some(CV_REF_ATTR.to_string()),
                accession: Some(format!("{}:9910001", CV_REF_ATTR)),
                name: "my-id".to_string(),
                value: None,
                unit_cv_ref: None,
                unit_name: None,
                unit_accession: None,
            },
            tag_id: TagId::CvParam as u8,
            id: 1,
            parent_id: 0,
        });
        buffer.normalize_attr_cv_values();
        assert_eq!(buffer.rows[0].cv_param.value.as_deref(), Some("my-id"));
        assert!(buffer.rows[0].cv_param.name.is_empty());
    }

    #[test]
    fn meta_param_buffer_normalize_skips_non_attr_cv() {
        let mut buffer = MetaParamBuffer::new();
        buffer.rows.push(MetaParamRow {
            cv_param: CvParam {
                cv_ref: Some("MS".to_string()),
                accession: Some("MS:1000514".to_string()),
                name: "m/z array".to_string(),
                value: None,
                unit_cv_ref: None,
                unit_name: None,
                unit_accession: None,
            },
            tag_id: TagId::CvParam as u8,
            id: 1,
            parent_id: 0,
        });
        buffer.normalize_attr_cv_values();
        assert_eq!(buffer.rows[0].cv_param.name, "m/z array");
        assert!(buffer.rows[0].cv_param.value.is_none());
    }

    #[test]
    fn packed_meta_builder_empty_produces_single_sentinel() {
        let meta = PackedMetaBuilder::new().build();
        assert_eq!(meta.index_offsets, vec![0]);
        assert!(meta.ids.is_empty());
    }

    #[test]
    fn packed_meta_builder_flush_buffer_advances_index_offsets() {
        let mut builder = PackedMetaBuilder::new();
        let mut buffer = MetaParamBuffer::new();
        buffer.push(TagId::CvParam, 1, 0, empty_cv_param());
        buffer.push(TagId::CvParam, 1, 0, empty_cv_param());
        builder.flush_buffer(&buffer);
        let meta = builder.build();
        assert_eq!(meta.index_offsets, vec![0, 2]);
        assert_eq!(meta.ids.len(), 2);
    }

    #[test]
    fn encode_user_param_with_value_uses_separator() {
        let p = UserParam {
            name: "my-param".to_string(),
            value: Some("42".to_string()),
            r#type: Some("xsd:float".to_string()),
            unit_cv_ref: None,
            unit_name: None,
            unit_accession: None,
        };
        let encoded = encode_user_param_as_cv(&p).value.unwrap();
        let parts: Vec<&str> = encoded.splitn(3, USER_PARAM_NAME_VALUE_SEPARATOR).collect();
        assert_eq!(parts[0], "my-param");
        assert_eq!(parts[1], "42");
        assert_eq!(parts[2], "xsd:float");
    }

    #[test]
    fn compress_bytes_if_enabled_level_zero_is_identity() {
        let input = vec![1u8, 2, 3, 4];
        assert_eq!(compress_bytes_if_enabled(input.clone(), 0), input);
    }

    #[test]
    fn array_type_accession_from_bda_returns_mz_array() {
        let bda = BinaryDataArray {
            cv_params: vec![CvParam {
                cv_ref: Some("MS".to_string()),
                accession: Some("MS:1000514".to_string()),
                name: "m/z array".to_string(),
                value: None,
                unit_cv_ref: None,
                unit_name: None,
                unit_accession: None,
            }],
            ..Default::default()
        };
        assert_eq!(array_type_accession_from_binary_data_array(&bda), MZ_ARRAY);
    }

    #[test]
    fn array_type_accession_from_bda_returns_zero_when_absent() {
        assert_eq!(
            array_type_accession_from_binary_data_array(&BinaryDataArray::default()),
            0
        );
    }

    #[test]
    fn collector_global_meta_on_empty_mzml_produces_run_buffer() {
        let mzml = MzML::default();
        let mut collector = MetaCollector::new();
        let (meta, counts) = collector.collect_global_meta(&mzml);
        assert_eq!(counts.n_run, 1);
        assert!(!meta.ids.is_empty());
    }

    #[test]
    fn grouped_metadata_keeps_values_local_to_each_group() {
        use crate::decoder::decode::MetadatumValue;
        use crate::ion::DecompressionBudget;
        use crate::ion::format::CODEC_NONE;
        use crate::ion::meta_groups::MetaTotals;
        use crate::ion::utilities::MetaGroupReader;

        let grouped = three_item_grouped();
        assert_eq!(grouped.group_count, 3);

        let mut reader = MetaGroupReader::new(
            Arc::from(grouped_bytes(&grouped)),
            grouped.group_count,
            1,
            3,
            MetaTotals {
                rows: 3,
                numeric: 3,
                string: 0,
                uncompressed: grouped.uncompressed_size,
            },
            CODEC_NONE,
            true,
            DecompressionBudget::default(),
            64 * 1024 * 1024,
        )
        .unwrap();

        let all = reader.read_all().unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].item_index, 0);
        assert_eq!(all[1].item_index, 1);
        assert_eq!(all[2].item_index, 2);
        assert_eq!(all[0].value, MetadatumValue::Number(10.5));
        assert_eq!(all[1].value, MetadatumValue::Number(20.5));
        assert_eq!(all[2].value, MetadatumValue::Number(30.5));

        let second = reader.read_item(1).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].item_index, 1);
        assert_eq!(second[0].value, MetadatumValue::Number(20.5));

        let third = reader.read_item(2).unwrap();
        assert_eq!(third.len(), 1);
        assert_eq!(third[0].value, MetadatumValue::Number(30.5));
    }

    fn three_item_grouped() -> GroupedSection {
        let make_cv = |value: &str| CvParam {
            cv_ref: Some("MS".to_string()),
            accession: Some("MS:1000285".to_string()),
            name: String::new(),
            value: Some(value.to_string()),
            unit_cv_ref: None,
            unit_name: None,
            unit_accession: None,
        };
        let mut grouper = MetaGrouper::new(1, 0, SectionChunk::memory(0));
        for (index, value) in ["10.5", "20.5", "30.5"].iter().enumerate() {
            let mut buffer = MetaParamBuffer::new();
            buffer.push(TagId::CvParam, (index + 1) as u32, 0, make_cv(value));
            buffer.normalize_attr_cv_values();
            grouper.write_metadata_item(&buffer).unwrap();
        }
        grouper.finish().unwrap()
    }

    fn grouped_bytes(grouped: &GroupedSection) -> &[u8] {
        grouped.section.as_slice().unwrap()
    }

    #[test]
    fn metadata_reader_rejects_wrong_uncompressed_total() {
        use crate::ion::DecompressionBudget;
        use crate::ion::format::CODEC_NONE;
        use crate::ion::meta_groups::MetaTotals;
        use crate::ion::utilities::MetaGroupReader;

        let grouped = three_item_grouped();
        let result = MetaGroupReader::new(
            Arc::from(grouped_bytes(&grouped)),
            grouped.group_count,
            1,
            3,
            MetaTotals {
                rows: 3,
                numeric: 3,
                string: 0,
                uncompressed: grouped.uncompressed_size + 1,
            },
            CODEC_NONE,
            true,
            DecompressionBudget::default(),
            64 * 1024 * 1024,
        );
        assert!(result.is_err());
    }

    #[test]
    fn metadata_reader_rejects_wrong_row_total() {
        use crate::ion::DecompressionBudget;
        use crate::ion::format::CODEC_NONE;
        use crate::ion::meta_groups::MetaTotals;
        use crate::ion::utilities::MetaGroupReader;

        let grouped = three_item_grouped();
        let reader = MetaGroupReader::new(
            Arc::from(grouped_bytes(&grouped)),
            grouped.group_count,
            1,
            3,
            MetaTotals {
                rows: 99,
                numeric: 3,
                string: 0,
                uncompressed: grouped.uncompressed_size,
            },
            CODEC_NONE,
            true,
            DecompressionBudget::default(),
            64 * 1024 * 1024,
        )
        .unwrap();
        assert!(reader.read_all().is_err());
    }

    #[test]
    fn metadata_reader_rejects_payload_into_directory() {
        use crate::ion::DecompressionBudget;
        use crate::ion::format::CODEC_NONE;
        use crate::ion::meta_groups::{META_GROUP_ENTRY_SIZE, MetaTotals};
        use crate::ion::utilities::MetaGroupReader;

        let grouped = three_item_grouped();
        let mut bytes = grouped_bytes(&grouped).to_vec();
        let directory_start = bytes.len() - grouped.group_count as usize * META_GROUP_ENTRY_SIZE;
        bytes[directory_start..directory_start + 8]
            .copy_from_slice(&(directory_start as u64).to_le_bytes());

        let mut reader = MetaGroupReader::new(
            Arc::from(bytes.as_slice()),
            grouped.group_count,
            1,
            3,
            MetaTotals {
                rows: 3,
                numeric: 3,
                string: 0,
                uncompressed: grouped.uncompressed_size,
            },
            CODEC_NONE,
            true,
            DecompressionBudget::default(),
            64 * 1024 * 1024,
        )
        .unwrap();
        assert!(reader.read_item(0).is_err());
    }

    #[test]
    fn array_policy_identifies_xy_arrays() {
        let policy = ArrayPolicy {
            x_array_accession: MZ_ARRAY,
            y_array_accession: INTENSITY_ARRAY,
            force_f32: true,
        };
        assert!(policy.is_xy_array(MZ_ARRAY));
        assert!(policy.is_xy_array(INTENSITY_ARRAY));
        assert!(!policy.is_xy_array(TIME_ARRAY));
        assert!(policy.should_force_f32(MZ_ARRAY));
    }

    #[test]
    fn array_policy_no_force_when_disabled() {
        let policy = ArrayPolicy {
            x_array_accession: MZ_ARRAY,
            y_array_accession: INTENSITY_ARRAY,
            force_f32: false,
        };
        assert!(!policy.should_force_f32(MZ_ARRAY));
    }

    #[test]
    fn group_local_node_ids_across_group_boundaries() {
        use crate::ion::encoder::encode::{
            DEFAULT_MIN_SPLIT_BYTES, DEFAULT_TARGET_SEGMENT_BYTES, EncodingConfig,
            TARGET_BLOCK_UNCOMPRESSED_BYTES,
        };
        use crate::ion::encoder::ion_writer::write_mzml_to_ion;
        use crate::ion::encoder::utilities::SectionChunkMode;
        use crate::mzml::structs::{
            BinaryData, BinaryDataArray, BinaryDataArrayList, CvParam, MzML, Run, Spectrum,
            SpectrumList,
        };

        fn make_array(accession: &str, data: Vec<f64>) -> BinaryDataArray {
            BinaryDataArray {
                cv_params: vec![CvParam {
                    cv_ref: Some("MS".to_string()),
                    accession: Some(accession.to_string()),
                    name: String::new(),
                    ..Default::default()
                }],
                binary: Some(BinaryData::F64(data)),
                ..Default::default()
            }
        }

        let mut spectra = Vec::new();
        for i in 0..9000 {
            let mz_data: Vec<f64> = (0..10).map(|j| 100.0 + i as f64 + j as f64 * 0.1).collect();
            let intensity_data: Vec<f64> = (0..10).map(|j| 1000.0 + i as f64 + j as f64).collect();

            let spectrum = Spectrum {
                id: format!("spectrum={}", i),
                index: Some(i as u32),
                binary_data_array_list: Some(BinaryDataArrayList {
                    count: Some(2),
                    binary_data_arrays: vec![
                        make_array("MS:1000514", mz_data),
                        make_array("MS:1000515", intensity_data),
                    ],
                }),
                ..Default::default()
            };
            spectra.push(spectrum);
        }

        let run = Run {
            id: "run1".to_string(),
            spectrum_list: Some(SpectrumList {
                count: Some(spectra.len()),
                spectra,
                ..Default::default()
            }),
            ..Default::default()
        };

        let mzml = MzML {
            run,
            ..Default::default()
        };

        let mut output = Vec::new();
        write_mzml_to_ion(
            &mzml,
            EncodingConfig {
                compression_level: 0,
                force_f32: false,
                uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
                parallel: false,
                section_chunk: SectionChunkMode::Memory,
                target_segment_bytes: DEFAULT_TARGET_SEGMENT_BYTES,
                min_split_bytes: DEFAULT_MIN_SPLIT_BYTES,
            },
            &mut output,
        )
        .unwrap();

        use crate::ion::decoder::decode::{Decoder, DecoderConfig, Metadatum};
        use crate::mzml::schema::TagId;

        fn find_row(rows: &[Metadatum], tag: TagId, id: u32) -> &Metadatum {
            rows.iter()
                .find(|row| row.tag_id == tag && row.id == id)
                .unwrap_or_else(|| panic!("row not found: tag={:?}, id={}", tag, id))
        }

        let mut decoder =
            Decoder::open(&output, DecoderConfig::default()).expect("failed to open decoder");

        let first_rows = decoder
            .spectrum_metadata_at(0)
            .expect("failed to read first spectrum metadata");
        let second_rows = decoder
            .spectrum_metadata_at(8192)
            .expect("failed to read second spectrum metadata");

        let first_list = find_row(&first_rows, TagId::SpectrumList, LOCAL_LIST_NODE_ID);
        let second_list = find_row(&second_rows, TagId::SpectrumList, LOCAL_LIST_NODE_ID);

        assert_eq!(
            first_list.parent_id, 0,
            "first group list should have parent_id=0"
        );
        assert_eq!(
            second_list.parent_id, 0,
            "second group list should have parent_id=0"
        );

        let first_spectrum = find_row(&first_rows, TagId::Spectrum, FIRST_LOCAL_ITEM_NODE_ID);
        let second_spectrum = find_row(&second_rows, TagId::Spectrum, FIRST_LOCAL_ITEM_NODE_ID);

        assert_eq!(
            first_spectrum.parent_id, LOCAL_LIST_NODE_ID,
            "first group spectrum should parent to list id=1"
        );
        assert_eq!(
            second_spectrum.parent_id, LOCAL_LIST_NODE_ID,
            "second group spectrum should parent to list id=1"
        );

        let first_bda_list = first_rows
            .iter()
            .find(|row| row.tag_id == TagId::BinaryDataArrayList)
            .expect("first group should have BinaryDataArrayList");
        let second_bda_list = second_rows
            .iter()
            .find(|row| row.tag_id == TagId::BinaryDataArrayList)
            .expect("second group should have BinaryDataArrayList");

        assert_eq!(
            first_bda_list.parent_id, FIRST_LOCAL_ITEM_NODE_ID,
            "first group BinaryDataArrayList should parent to spectrum id=2"
        );
        assert_eq!(
            second_bda_list.parent_id, FIRST_LOCAL_ITEM_NODE_ID,
            "second group BinaryDataArrayList should parent to spectrum id=2"
        );
    }
}
