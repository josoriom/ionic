use crate::encoder::encode::FILTER_INDEX_RECORD_SIZE;
use crate::ion::encoder::utilities::container_builder::FilterType;
use crate::ion::utilities::spectrum_source::ScanMeta;
use crate::ion::utilities::{
    container_view::{ContainerView, DefaultProcessor},
    parse_header::{Header, parse_header},
    spectrum_source::{SpectrumSource, f16_bits_to_f64},
};
use crate::mzml::structs::FilterRecord;

use crate::{
    decoder::decode::{Metadatum, slice_at},
    ion::{
        attr_meta::{
            ACC_ATTR_DEFAULT_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_ID,
            ACC_ATTR_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_REF, ACC_ATTR_SAMPLE_REF,
            ACC_ATTR_START_TIME_STAMP, parse_accession_tail,
        },
        utilities::{
            children_lookup::{ChildrenLookup, DefaultMetadataPolicy, OwnerRows},
            common::get_attr_text,
            parse_chromatogram_list, parse_cv_and_user_params, parse_cv_list,
            parse_data_processing_list, parse_file_description,
            parse_global_metadata::parse_global_metadata,
            parse_instrument_list, parse_metadata, parse_referenceable_param_group_list,
            parse_sample_list, parse_scan_settings_list, parse_software_list, parse_spectrum_list,
        },
    },
    mzml::{schema::TagId, structs::*},
};

const ACC_MZ: u32 = 1_000_514;
const ACC_INT: u32 = 1_000_515;
const ENTRY_A_BYTES: usize = 16;
const ENTRY_A1_BYTES: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct ArrayRef {
    pub block_id: u32,
    pub element_offset: u64,
    pub element_count: u64,
    pub array_type: u32,
    pub dtype: u8,
}

pub struct IonReader<'a> {
    bytes: &'a [u8],
    header: Header,
    spec_container: ContainerView<'a, DefaultProcessor>,
    chrom_container: Option<ContainerView<'a, DefaultProcessor>>,
    mz_buf: Vec<f64>,
    int_buf: Vec<f64>,
}

impl<'a> IonReader<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Self, String> {
        let header = parse_header(bytes)?;
        let filter = FilterType::try_from(header.array_filter).unwrap_or(FilterType::None);

        let spec_container = {
            let off = header.off_container_spect as usize;
            let len = header.len_container_spect as usize;
            let container_bytes = bytes
                .get(off..off + len)
                .ok_or("spectrum container out of bounds")?;
            ContainerView::new(
                container_bytes,
                header.block_count_spect,
                header.compression_level,
                filter,
                "spec",
                DefaultProcessor,
            )?
        };

        let chrom_container = if header.block_count_chrom > 0 && header.len_container_chrom > 0 {
            let off = header.off_container_chrom as usize;
            let len = header.len_container_chrom as usize;
            let container_bytes = bytes
                .get(off..off + len)
                .ok_or("chrom container out of bounds")?;
            Some(ContainerView::new(
                container_bytes,
                header.block_count_chrom,
                header.compression_level,
                filter,
                "chrom",
                DefaultProcessor,
            )?)
        } else {
            None
        };

        Ok(Self {
            bytes,
            header,
            spec_container,
            chrom_container,
            mz_buf: Vec::new(),
            int_buf: Vec::new(),
        })
    }

    #[inline]
    pub fn header(&self) -> &Header {
        &self.header
    }

    #[inline]
    pub fn spectrum_count(&self) -> u64 {
        self.header.spectrum_count
    }

    #[inline]
    pub fn chromatogram_count(&self) -> u64 {
        self.header.chrom_count
    }

    pub fn filter_record(&self, index: usize) -> Option<FilterRecord> {
        let base = self.header.off_filter_index as usize + index * FILTER_INDEX_RECORD_SIZE;
        let b = self.bytes.get(base..base + FILTER_INDEX_RECORD_SIZE)?;
        Some(parse_filter_record(b))
    }

    pub fn filter_records(&self) -> Result<Vec<FilterRecord>, String> {
        let off = self.header.off_filter_index as usize;
        let len = self.header.len_filter_index as usize;
        let count = self.header.spectrum_count as usize;

        if len != count * FILTER_INDEX_RECORD_SIZE {
            return Err(format!(
                "filter index: len_filter_index={len} != spectrum_count={count} × {FILTER_INDEX_RECORD_SIZE}"
            ));
        }

        let section = self
            .bytes
            .get(off..off + len)
            .ok_or_else(|| "filter index: section out of bounds".to_string())?;

        let mut records = Vec::with_capacity(count);
        for i in 0..count {
            let base = i * FILTER_INDEX_RECORD_SIZE;
            records.push(parse_filter_record(
                &section[base..base + FILTER_INDEX_RECORD_SIZE],
            ));
        }
        Ok(records)
    }

    pub fn spectrum_array_refs(&self, index: usize) -> Option<Vec<ArrayRef>> {
        if index >= self.header.spectrum_count as usize {
            return None;
        }
        read_array_refs_at(
            self.bytes,
            self.header.off_spec_entries as usize,
            self.header.off_spec_arrayrefs as usize,
            index,
        )
    }

    pub fn chromatogram_array_refs(&self, index: usize) -> Option<Vec<ArrayRef>> {
        if index >= self.header.chrom_count as usize {
            return None;
        }
        read_array_refs_at(
            self.bytes,
            self.header.off_chrom_entries as usize,
            self.header.off_chrom_arrayrefs as usize,
            index,
        )
    }

    pub fn read_spectrum_array(&mut self, aref: &ArrayRef) -> Result<Vec<f64>, String> {
        let stride = dtype_stride(aref.dtype);
        let raw = self.spec_container.get_item_from_block(
            aref.block_id,
            aref.element_offset,
            aref.element_count,
            stride,
            "read_spectrum_array",
        )?;
        let mut buf = Vec::new();
        decode_into(&mut buf, raw, aref.dtype);
        Ok(buf)
    }

    pub fn read_chromatogram_array(&mut self, aref: &ArrayRef) -> Result<Vec<f64>, String> {
        let container = self
            .chrom_container
            .as_mut()
            .ok_or_else(|| "no chromatogram container".to_string())?;
        let stride = dtype_stride(aref.dtype);
        let raw = container.get_item_from_block(
            aref.block_id,
            aref.element_offset,
            aref.element_count,
            stride,
            "read_chromatogram_array",
        )?;
        let mut buf = Vec::new();
        decode_into(&mut buf, raw, aref.dtype);
        Ok(buf)
    }

    pub fn global_metadata(&self) -> Result<Vec<Metadatum>, String> {
        let s = slice_at(
            self.bytes,
            self.header.off_global_meta,
            self.header.len_global_meta,
            "global",
        )?;
        parse_global_metadata(
            s,
            0,
            self.header.global_meta_count,
            self.header.global_meta_num_count,
            self.header.global_meta_str_count,
            self.header.compression_codec,
            self.header.global_meta_uncompressed_bytes,
        )
    }

    pub fn spectrum_metadata(&self) -> Result<Vec<Metadatum>, String> {
        let s = slice_at(
            self.bytes,
            self.header.off_spec_meta,
            self.header.len_spec_meta,
            "spec_meta",
        )?;
        parse_metadata(
            s,
            self.header.spectrum_count,
            self.header.spec_meta_count,
            self.header.spec_meta_num_count,
            self.header.spec_meta_str_count,
            self.header.compression_codec,
            self.header.spec_meta_uncompressed_bytes as usize,
        )
    }

    pub fn chromatogram_metadata(&self) -> Result<Vec<Metadatum>, String> {
        let s = slice_at(
            self.bytes,
            self.header.off_chrom_meta,
            self.header.len_chrom_meta,
            "chrom_meta",
        )?;
        parse_metadata(
            s,
            self.header.chrom_count,
            self.header.chrom_meta_count,
            self.header.chrom_meta_num_count,
            self.header.chrom_meta_str_count,
            self.header.compression_codec,
            self.header.chrom_meta_uncompressed_bytes as usize,
        )
    }

    #[deprecated(
        note = "use the lazy API (spectrum_array_refs, read_spectrum_array, etc.) instead"
    )]
    pub fn to_mzml(&mut self) -> Result<MzML, String> {
        let global_meta = self.global_metadata()?;
        let global_lookup = ChildrenLookup::new(&global_meta);
        let meta_refs: Vec<&Metadatum> = global_meta.iter().collect();
        let policy = DefaultMetadataPolicy;

        let mut owner_rows = OwnerRows::with_capacity(global_meta.len());
        for m in &global_meta {
            owner_rows.insert(m.id, m);
        }

        let run_id = global_lookup
            .all_ids(TagId::Run)
            .first()
            .copied()
            .unwrap_or(0);

        let rows = owner_rows.get(run_id);

        let mut param_buffer: Vec<&Metadatum> = Vec::new();
        global_lookup.get_param_rows_into(&owner_rows, run_id, &policy, &mut param_buffer);
        let (cv_params, user_params) = parse_cv_and_user_params(&param_buffer);

        let spec_meta = self.spectrum_metadata()?;
        let chrom_meta = self.chromatogram_metadata()?;
        let spec_refs: Vec<&Metadatum> = spec_meta.iter().collect();
        let chrom_refs: Vec<&Metadatum> = chrom_meta.iter().collect();

        let mut spectrum_list =
            parse_spectrum_list(&spec_refs, &ChildrenLookup::new(&spec_meta), &policy);
        let mut chromatogram_list =
            parse_chromatogram_list(&chrom_refs, &ChildrenLookup::new(&chrom_meta), &policy);

        if let Some(sl) = spectrum_list.as_mut() {
            self.attach_spec_binaries(&mut sl.spectra)?;
        }

        if let Some(cl) = chromatogram_list.as_mut() {
            self.attach_chrom_binaries(&mut cl.chromatograms)?;
        }

        let source_file_ref_list = parse_run_source_file_refs(&owner_rows, &global_lookup, run_id);

        let filter_record = self.filter_records()?;

        let run = Run {
            id: get_attr_text(rows, ACC_ATTR_ID).unwrap_or_default(),
            start_time_stamp: get_attr_text(rows, ACC_ATTR_START_TIME_STAMP)
                .filter(|s| !s.is_empty()),
            default_instrument_configuration_ref: get_attr_text(
                rows,
                ACC_ATTR_DEFAULT_INSTRUMENT_CONFIGURATION_REF,
            )
            .or_else(|| get_attr_text(rows, ACC_ATTR_INSTRUMENT_CONFIGURATION_REF)),
            sample_ref: get_attr_text(rows, ACC_ATTR_SAMPLE_REF),
            cv_params,
            user_params,
            source_file_ref_list,
            spectrum_list,
            chromatogram_list,
            ..Default::default()
        };

        Ok(MzML {
            cv_list: parse_cv_list(&meta_refs, &global_lookup),
            file_description: parse_file_description(&meta_refs, &global_lookup, &policy),
            referenceable_param_group_list: parse_referenceable_param_group_list(
                &meta_refs,
                &global_lookup,
                &policy,
            ),
            sample_list: parse_sample_list(&meta_refs, &global_lookup, &policy),
            instrument_list: parse_instrument_list(&meta_refs, &global_lookup, &policy),
            software_list: parse_software_list(&meta_refs, &global_lookup, &policy),
            data_processing_list: parse_data_processing_list(&meta_refs, &global_lookup, &policy),
            scan_settings_list: parse_scan_settings_list(&meta_refs, &global_lookup, &policy),
            run,
            filter_record,
        })
    }

    fn attach_spec_binaries(&mut self, spectra: &mut [Spectrum]) -> Result<(), String> {
        let entry_base = self.header.off_spec_entries as usize;
        let aref_base = self.header.off_spec_arrayrefs as usize;

        for (i, spectrum) in spectra.iter_mut().enumerate() {
            let Some(refs) = read_array_refs_at(self.bytes, entry_base, aref_base, i) else {
                continue;
            };
            if refs.is_empty() {
                continue;
            }
            let bdal = spectrum
                .binary_data_array_list
                .get_or_insert_with(BinaryDataArrayList::default);
            for aref in &refs {
                let stride = dtype_stride(aref.dtype);
                let raw = self.spec_container.get_item_from_block(
                    aref.block_id,
                    aref.element_offset,
                    aref.element_count,
                    stride,
                    "spec",
                )?;
                attach_array(bdal, aref.array_type, aref.dtype, raw);
            }
            bdal.count = Some(bdal.binary_data_arrays.len());
        }
        Ok(())
    }

    fn attach_chrom_binaries(&mut self, chromatograms: &mut [Chromatogram]) -> Result<(), String> {
        let container = match self.chrom_container.as_mut() {
            Some(c) => c,
            None => return Ok(()),
        };
        let entry_base = self.header.off_chrom_entries as usize;
        let aref_base = self.header.off_chrom_arrayrefs as usize;

        for (i, chromatogram) in chromatograms.iter_mut().enumerate() {
            let Some(refs) = read_array_refs_at(self.bytes, entry_base, aref_base, i) else {
                continue;
            };
            if refs.is_empty() {
                continue;
            }
            let bdal = chromatogram
                .binary_data_array_list
                .get_or_insert_with(BinaryDataArrayList::default);
            for aref in &refs {
                let stride = dtype_stride(aref.dtype);
                let raw = container.get_item_from_block(
                    aref.block_id,
                    aref.element_offset,
                    aref.element_count,
                    stride,
                    "chrom",
                )?;
                attach_array(bdal, aref.array_type, aref.dtype, raw);
            }
            bdal.count = Some(bdal.binary_data_arrays.len());
        }
        Ok(())
    }
}

impl<'a> SpectrumSource for IonReader<'a> {
    fn for_each_scan_in_range(
        &mut self,
        rt_min: f64,
        rt_max: f64,
        ms_level: u8,
        callback: &mut dyn FnMut(f64, &ScanMeta, &[f64], &[f64]),
    ) {
        let rt_min_s = rt_min * 60.0;
        let rt_max_s = rt_max * 60.0;
        let count = self.header.spectrum_count as usize;
        let filter_base = self.header.off_filter_index as usize;
        let entry_base = self.header.off_spec_entries as usize;
        let aref_base = self.header.off_spec_arrayrefs as usize;

        for i in 0..count {
            let fb = filter_base + i * FILTER_INDEX_RECORD_SIZE;
            let Some(fs) = self.bytes.get(fb..fb + FILTER_INDEX_RECORD_SIZE) else {
                continue;
            };

            let rt_s = f64::from_le_bytes(fs[0..8].try_into().unwrap());
            if !rt_s.is_finite() || rt_s < rt_min_s || rt_s > rt_max_s {
                continue;
            }

            let ms = fs[40];
            if ms_level != 0 && ms != ms_level {
                continue;
            }

            let ea = entry_base + i * ENTRY_A_BYTES;
            let Some(es) = self.bytes.get(ea..ea + ENTRY_A_BYTES) else {
                continue;
            };
            let ref_start = u64::from_le_bytes(es[0..8].try_into().unwrap()) as usize;
            let ref_count = u64::from_le_bytes(es[8..16].try_into().unwrap()) as usize;

            let mut mz_ref: Option<ArrayRef> = None;
            let mut int_ref: Option<ArrayRef> = None;

            for j in 0..ref_count {
                let ab = aref_base + (ref_start + j) * ENTRY_A1_BYTES;
                let Some(ar) = self.bytes.get(ab..ab + ENTRY_A1_BYTES) else {
                    break;
                };
                let aref = parse_array_ref(ar);
                match aref.array_type {
                    ACC_MZ => mz_ref = Some(aref),
                    ACC_INT => int_ref = Some(aref),
                    _ => {}
                }
                if mz_ref.is_some() && int_ref.is_some() {
                    break;
                }
            }

            let (Some(mr), Some(ir)) = (mz_ref, int_ref) else {
                continue;
            };
            if !decode_from_block(&mut self.spec_container, &mut self.mz_buf, &mr) {
                continue;
            }
            if !decode_from_block(&mut self.spec_container, &mut self.int_buf, &ir) {
                continue;
            }

            let n = self.mz_buf.len().min(self.int_buf.len());
            if n == 0 {
                continue;
            }

            let meta = ScanMeta {
                ms_level: ms,
                polarity: fs[41],
                base_peak_mz: f64::from_le_bytes(fs[8..16].try_into().unwrap()),
                selected_ion_mz: f64::from_le_bytes(fs[16..24].try_into().unwrap()),
                base_peak_int: f64::from_le_bytes(fs[24..32].try_into().unwrap()),
                total_ion_current: f64::from_le_bytes(fs[32..40].try_into().unwrap()),
            };

            callback(rt_s / 60.0, &meta, &self.mz_buf[..n], &self.int_buf[..n]);
        }
    }
}

#[inline]
fn parse_filter_record(b: &[u8]) -> FilterRecord {
    FilterRecord {
        rt_seconds: f64::from_le_bytes(b[0..8].try_into().unwrap()),
        base_peak_mz: f64::from_le_bytes(b[8..16].try_into().unwrap()),
        selected_ion_mz: f64::from_le_bytes(b[16..24].try_into().unwrap()),
        base_peak_int: f64::from_le_bytes(b[24..32].try_into().unwrap()),
        total_ion_current: f64::from_le_bytes(b[32..40].try_into().unwrap()),
        ms_level: b[40],
        polarity: b[41],
    }
}

#[inline]
fn read_array_refs_at(
    bytes: &[u8],
    entry_base: usize,
    aref_base: usize,
    index: usize,
) -> Option<Vec<ArrayRef>> {
    let ea = entry_base + index * ENTRY_A_BYTES;
    let entry = bytes.get(ea..ea + ENTRY_A_BYTES)?;
    let ref_start = u64::from_le_bytes(entry[0..8].try_into().unwrap()) as usize;
    let ref_count = u64::from_le_bytes(entry[8..16].try_into().unwrap()) as usize;

    let mut refs = Vec::with_capacity(ref_count);
    for j in 0..ref_count {
        let ab = aref_base + (ref_start + j) * ENTRY_A1_BYTES;
        let b = bytes.get(ab..ab + ENTRY_A1_BYTES)?;
        refs.push(parse_array_ref(b));
    }
    Some(refs)
}

#[inline]
fn parse_array_ref(b: &[u8]) -> ArrayRef {
    ArrayRef {
        element_offset: u64::from_le_bytes(b[0..8].try_into().unwrap()),
        element_count: u64::from_le_bytes(b[8..16].try_into().unwrap()),
        block_id: u32::from_le_bytes(b[16..20].try_into().unwrap()),
        array_type: u32::from_le_bytes(b[20..24].try_into().unwrap()),
        dtype: b[24],
    }
}

#[inline]
fn decode_from_block(
    container: &mut ContainerView<'_, DefaultProcessor>,
    buf: &mut Vec<f64>,
    aref: &ArrayRef,
) -> bool {
    let stride = dtype_stride(aref.dtype);
    let raw = match container.get_item_from_block(
        aref.block_id,
        aref.element_offset,
        aref.element_count,
        stride,
        "scan",
    ) {
        Ok(r) => r,
        Err(_) => return false,
    };
    decode_into(buf, raw, aref.dtype);
    true
}

#[inline]
fn dtype_stride(dtype: u8) -> usize {
    match dtype {
        1 | 6 => 8,
        2 | 5 => 4,
        3 | 4 => 2,
        _ => 1,
    }
}

fn decode_into(buf: &mut Vec<f64>, raw: &[u8], dtype: u8) {
    buf.clear();
    match dtype {
        1 => {
            let n = raw.len() / 8;
            buf.reserve(n);
            unsafe {
                buf.set_len(n);
                std::ptr::copy_nonoverlapping(raw.as_ptr(), buf.as_mut_ptr() as *mut u8, raw.len());
            }
        }
        2 => buf.extend(
            raw.chunks_exact(4)
                .map(|c| f32::from_le_bytes(c.try_into().unwrap()) as f64),
        ),
        3 => buf.extend(
            raw.chunks_exact(2)
                .map(|c| f16_bits_to_f64(u16::from_le_bytes(c.try_into().unwrap()))),
        ),
        4 => buf.extend(
            raw.chunks_exact(2)
                .map(|c| i16::from_le_bytes(c.try_into().unwrap()) as f64),
        ),
        5 => buf.extend(
            raw.chunks_exact(4)
                .map(|c| i32::from_le_bytes(c.try_into().unwrap()) as f64),
        ),
        6 => buf.extend(
            raw.chunks_exact(8)
                .map(|c| i64::from_le_bytes(c.try_into().unwrap()) as f64),
        ),
        _ => {}
    }
}

fn attach_array(bdal: &mut BinaryDataArrayList, array_type: u32, dtype: u8, raw: &[u8]) {
    let binary = raw_to_binary_data(raw, dtype);
    let numeric_type = dtype_to_numeric_type(dtype);

    let found = bdal
        .binary_data_arrays
        .iter_mut()
        .find(|b| bda_matches(b, array_type));

    let bda = match found {
        Some(existing) => existing,
        None => {
            bdal.binary_data_arrays.push(make_bda_stub(array_type));
            bdal.binary_data_arrays.last_mut().unwrap()
        }
    };

    bda.binary = Some(binary);
    sync_numeric_meta(bda, numeric_type);
}

fn raw_to_binary_data(raw: &[u8], dtype: u8) -> BinaryData {
    match dtype {
        1 => BinaryData::F64(byte_cast(raw)),
        2 => BinaryData::F32(byte_cast(raw)),
        3 => BinaryData::F16(byte_cast(raw)),
        4 => BinaryData::I16(byte_cast(raw)),
        5 => BinaryData::I32(byte_cast(raw)),
        6 => BinaryData::I64(byte_cast(raw)),
        _ => BinaryData::F64(Vec::new()),
    }
}

#[inline]
fn byte_cast<T>(raw: &[u8]) -> Vec<T> {
    let element_count = raw.len() / std::mem::size_of::<T>();
    let mut out = Vec::with_capacity(element_count);
    unsafe {
        out.set_len(element_count);
        std::ptr::copy_nonoverlapping(raw.as_ptr(), out.as_mut_ptr() as *mut u8, raw.len());
    }
    out
}

fn dtype_to_numeric_type(dtype: u8) -> NumericType {
    match dtype {
        1 => NumericType::Float64,
        2 => NumericType::Float32,
        3 => NumericType::Float16,
        4 => NumericType::Int16,
        5 => NumericType::Int32,
        6 => NumericType::Int64,
        _ => NumericType::Float64,
    }
}

fn make_bda_stub(array_type_accession: u32) -> BinaryDataArray {
    BinaryDataArray {
        cv_params: vec![CvParam {
            cv_ref: Some("MS".to_string()),
            accession: Some(format!("MS:{array_type_accession:07}")),
            name: String::new(),
            value: None,
            unit_cv_ref: None,
            unit_name: None,
            unit_accession: None,
        }],
        ..BinaryDataArray::default()
    }
}

#[inline]
fn sync_numeric_meta(bda: &mut BinaryDataArray, numeric_type: NumericType) {
    let target = match numeric_type {
        NumericType::Float16 => 1_000_520,
        NumericType::Float32 => 1_000_521,
        NumericType::Float64 => 1_000_523,
        NumericType::Int16 => 1_000_518,
        NumericType::Int32 => 1_000_519,
        NumericType::Int64 => 1_000_522,
    };
    bda.cv_params
        .retain(|p| !is_numeric_acc(parse_accession_tail(p.accession.as_deref())));
    bda.cv_params.push(ms_numeric_param(target));
    bda.numeric_type = Some(numeric_type);
}

#[inline]
fn is_numeric_acc(tail: crate::ion::attr_meta::AccessionTail) -> bool {
    matches!(
        tail.raw(),
        1_000_520 | 1_000_521 | 1_000_523 | 1_000_518 | 1_000_519 | 1_000_522
    )
}

#[inline]
fn bda_matches(bda: &BinaryDataArray, kind: u32) -> bool {
    bda.cv_params
        .iter()
        .any(|p| parse_accession_tail(p.accession.as_deref()).raw() == kind)
}

#[inline]
fn ms_numeric_param(tail: u32) -> CvParam {
    let name = match tail {
        1_000_521 => "32-bit float",
        1_000_523 => "64-bit float",
        1_000_519 => "32-bit integer",
        1_000_522 => "64-bit integer",
        _ => "numeric",
    };
    CvParam {
        cv_ref: Some("MS".into()),
        accession: Some(format!("MS:{tail:07}")),
        name: name.into(),
        ..Default::default()
    }
}

fn parse_run_source_file_refs(
    owner_rows: &OwnerRows,
    lookup: &ChildrenLookup,
    run_id: u32,
) -> Option<SourceFileRefList> {
    let list_id = lookup
        .ids_for(run_id, TagId::SourceFileRefList)
        .first()
        .copied()
        .or_else(|| lookup.all_ids(TagId::SourceFileRefList).first().copied())?;

    let refs: Vec<_> = lookup
        .ids_for(list_id, TagId::SourceFileRef)
        .iter()
        .filter_map(|&ref_id| {
            get_attr_text(owner_rows.get(ref_id), ACC_ATTR_REF).map(|r| SourceFileRef { r#ref: r })
        })
        .collect();

    (!refs.is_empty()).then(|| SourceFileRefList {
        count: Some(refs.len()),
        source_file_refs: refs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BYTES: &[u8] = include_bytes!("../../../../data/ion/test.ion");

    #[test]
    fn open_parses_header() {
        let reader = IonReader::open(BYTES).unwrap();
        assert!(reader.spectrum_count() > 0);
    }

    #[test]
    fn filter_record_returns_none_out_of_bounds() {
        let reader = IonReader::open(BYTES).unwrap();
        assert!(
            reader
                .filter_record(reader.spectrum_count() as usize)
                .is_none()
        );
    }

    #[test]
    fn filter_record_has_valid_rt() {
        let reader = IonReader::open(BYTES).unwrap();
        let r = reader.filter_record(0).unwrap();
        assert!(r.rt_seconds.is_finite() && r.rt_seconds >= 0.0);
        assert!(r.ms_level >= 1);
    }

    #[test]
    fn array_refs_contain_mz_and_intensity() {
        let reader = IonReader::open(BYTES).unwrap();
        let refs = reader.spectrum_array_refs(0).unwrap();
        assert!(refs.iter().any(|a| a.array_type == ACC_MZ));
        assert!(refs.iter().any(|a| a.array_type == ACC_INT));
    }

    #[test]
    fn read_spectrum_array_produces_mz_values() {
        let mut reader = IonReader::open(BYTES).unwrap();
        let refs = reader.spectrum_array_refs(0).unwrap();
        let mz_ref = refs.iter().find(|a| a.array_type == ACC_MZ).unwrap();
        let mz = reader.read_spectrum_array(mz_ref).unwrap();
        assert!(!mz.is_empty());
        assert!(mz.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn for_each_scan_yields_matching_scans() {
        let mut reader = IonReader::open(BYTES).unwrap();
        let mut count = 0usize;
        reader.for_each_scan_in_range(0.0, f64::MAX, 0, &mut |rt, _meta, mz, int| {
            assert!(rt.is_finite());
            assert!(!mz.is_empty());
            assert_eq!(mz.len(), int.len());
            count += 1;
        });
        assert_eq!(count, reader.spectrum_count() as usize);
    }

    #[test]
    fn for_each_scan_filters_by_ms_level() {
        let mut reader = IonReader::open(BYTES).unwrap();
        let mut count = 0usize;
        reader.for_each_scan_in_range(0.0, f64::MAX, 1, &mut |_, _, _, _| {
            count += 1;
        });
        let expected = (0..reader.spectrum_count() as usize)
            .filter(|&i| reader.filter_record(i).map_or(false, |r| r.ms_level == 1))
            .count();
        assert_eq!(count, expected);
    }

    #[allow(deprecated)]
    #[test]
    fn to_mzml_produces_valid_structure() {
        let mut reader = IonReader::open(BYTES).unwrap();
        let mzml = reader.to_mzml().unwrap();
        let sl = mzml.run.spectrum_list.as_ref().unwrap();
        assert!(!sl.spectra.is_empty());
        let first = &sl.spectra[0];
        assert!(first.binary_data_array_list.is_some());
        let bdal = first.binary_data_array_list.as_ref().unwrap();
        assert!(!bdal.binary_data_arrays.is_empty());
        assert!(bdal.binary_data_arrays.iter().any(|b| b.binary.is_some()));
    }

    #[allow(deprecated)]
    #[test]
    fn to_mzml_filter_records_match_count() {
        let mut reader = IonReader::open(BYTES).unwrap();
        let mzml = reader.to_mzml().unwrap();
        assert_eq!(
            mzml.filter_record.len(),
            mzml.run
                .spectrum_list
                .as_ref()
                .map(|sl| sl.spectra.len())
                .unwrap_or(0)
        );
    }

    #[test]
    fn global_metadata_returns_entries() {
        let reader = IonReader::open(BYTES).unwrap();
        let meta = reader.global_metadata().unwrap();
        assert!(!meta.is_empty());
    }

    #[test]
    fn spectrum_metadata_returns_entries() {
        let reader = IonReader::open(BYTES).unwrap();
        let meta = reader.spectrum_metadata().unwrap();
        assert!(!meta.is_empty());
    }

    #[test]
    fn decode_into_f64_roundtrips() {
        let vals = [1.5f64, 2.5, 3.5];
        let raw: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut buf = Vec::new();
        decode_into(&mut buf, &raw, 1);
        assert_eq!(buf, vals);
    }

    #[test]
    fn decode_into_f32_converts() {
        let vals = [1.0f32, 2.0];
        let raw: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut buf = Vec::new();
        decode_into(&mut buf, &raw, 2);
        assert_eq!(buf.len(), 2);
        assert!((buf[0] - 1.0).abs() < f64::EPSILON);
        assert!((buf[1] - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn decode_into_reuses_buffer() {
        let raw = 42.0f64.to_le_bytes();
        let mut buf = Vec::with_capacity(1024);
        decode_into(&mut buf, &raw, 1);
        assert_eq!(buf.len(), 1);
        let cap = buf.capacity();
        decode_into(&mut buf, &raw, 1);
        assert_eq!(buf.capacity(), cap);
    }

    #[test]
    fn dtype_stride_maps_all_types() {
        assert_eq!(dtype_stride(1), 8);
        assert_eq!(dtype_stride(2), 4);
        assert_eq!(dtype_stride(3), 2);
        assert_eq!(dtype_stride(4), 2);
        assert_eq!(dtype_stride(5), 4);
        assert_eq!(dtype_stride(6), 8);
    }
}
