use crate::{
    encoder::utilities::le_writers::{write_f64_slice_le, write_u32_slice_le},
    ion::{IonResult, meta_groups::{META_GROUP_ENTRY_SIZE, MetaGroupEntry, write_group_header}},
};

use super::{
    MetaParamBuffer, MetadataWriter, PackedMeta, PackedMetaBuilder, compress_bytes_if_enabled,
};
use super::super::sink::SectionChunk;

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

pub(super) fn serialize_group(meta: &PackedMeta, item_start: usize, item_end: usize) -> Vec<u8> {
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

pub(crate) fn serialize_global_meta_with_counts(counts: &super::GlobalCounts, m: &PackedMeta) -> Vec<u8> {
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
