use crate::{
    accessions::{
        FLOAT_32BIT, FLOAT_64BIT, HIGHEST_ION_MOBILITY, HIGHEST_OBSERVED_MZ,
        HIGHEST_OBSERVED_WAVELENGTH, INTENSITY_ARRAY, LOWEST_ION_MOBILITY, LOWEST_OBSERVED_MZ,
        LOWEST_OBSERVED_WAVELENGTH, MZ_ARRAY, NEGATIVE_SCAN, POSITIVE_SCAN, TIME_ARRAY,
    },
    encoder::utilities::{
        FileHeader,
        encoder_output::EncoderOutput,
        le_writers::{
            write_f32_le, write_f32_slice_le, write_f64_le, write_f64_slice_le, write_i16_slice_le,
            write_i32_slice_le, write_i64_slice_le, write_u16_slice_le, write_u32_le, write_u64_le,
        },
        meta_collector::{
            ArrayPolicy, CompressedMetaSections, MetaCollector, PackedMeta,
            array_type_accession_from_binary_data_array, parse_accession_tail_raw,
        },
    },
    ion::{
        IonError, IonResult,
        encoder::utilities::{CompressionMode, ContainerBuilder, DefaultCompressor},
        filter_summary::{ChromatogramSummary, SpectrumSummary},
        packing::raw::RAW as RAW_PACKING,
        packing::{Dtype, Packing, PackingId, PackingInput, packing_for},
        utilities::spectrum_source::summary_from_spectrum,
    },
    mzml::structs::{
        BinaryData, BinaryDataArray, BinaryDataArrayList, Chromatogram, CvParam, MzML, NumericType,
        Spectrum,
    },
};

use crate::ion::format::{FILE_TRAILER, HEADER_SIZE};
pub const TARGET_BLOCK_UNCOMPRESSED_BYTES: usize = 1 * 1024 * 1024;
pub(crate) const SPEC_SUMMARY_SIZE: usize = 128;
pub(crate) const CHROM_SUMMARY_SIZE: usize = 128;

pub(crate) const FILE_DTYPE_F64: u8 = 1;
pub(crate) const FILE_DTYPE_F32: u8 = 2;
pub(crate) const FILE_DTYPE_F16: u8 = 3;
pub(crate) const FILE_DTYPE_I16: u8 = 4;
pub(crate) const FILE_DTYPE_I32: u8 = 5;
pub(crate) const FILE_DTYPE_I64: u8 = 6;

const POLARITY_UNKNOWN: u8 = 0;
const POLARITY_POSITIVE: u8 = 1;
const POLARITY_NEGATIVE: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritingMode {
    Memory,
    Streaming,
}

impl SpectrumSummary {
    #[cfg(test)]
    fn unknown() -> Self {
        Self {
            rt_seconds: f64::NAN,
            base_peak_mz: f64::NAN,
            selected_ion_mz: f64::NAN,
            base_peak_int: f64::NAN,
            total_ion_current: f64::NAN,
            ms_level: 0,
            polarity: POLARITY_UNKNOWN,
        }
    }

    fn write_into(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.rt_seconds.to_le_bytes());
        buf.extend_from_slice(&self.base_peak_mz.to_le_bytes());
        buf.extend_from_slice(&self.selected_ion_mz.to_le_bytes());
        buf.extend_from_slice(&self.base_peak_int.to_le_bytes());
        buf.extend_from_slice(&self.total_ion_current.to_le_bytes());
        buf.push(self.ms_level);
        buf.push(self.polarity);
        buf.extend_from_slice(&[0u8; 86]);
    }
}

impl ChromatogramSummary {
    fn unknown() -> Self {
        Self {
            lowest_mz: f64::NAN,
            highest_mz: f64::NAN,
            lowest_wavelength: f64::NAN,
            highest_wavelength: f64::NAN,
            lowest_ion_mobility: f64::NAN,
            highest_ion_mobility: f64::NAN,
            polarity: POLARITY_UNKNOWN,
        }
    }

    fn write_into(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.lowest_mz.to_le_bytes());
        buf.extend_from_slice(&self.highest_mz.to_le_bytes());
        buf.extend_from_slice(&self.lowest_wavelength.to_le_bytes());
        buf.extend_from_slice(&self.highest_wavelength.to_le_bytes());
        buf.extend_from_slice(&self.lowest_ion_mobility.to_le_bytes());
        buf.extend_from_slice(&self.highest_ion_mobility.to_le_bytes());
        buf.push(self.polarity);
        buf.extend_from_slice(&[0u8; 79]);
    }
}

fn spec_summary_from_spectrum(spectrum: &Spectrum) -> SpectrumSummary {
    let summary = summary_from_spectrum(spectrum);
    SpectrumSummary {
        rt_seconds: summary.rt * 60.0,
        base_peak_mz: summary.base_peak_mz,
        selected_ion_mz: summary.selected_ion_mz,
        base_peak_int: summary.base_peak_int,
        total_ion_current: summary.total_ion_current,
        ms_level: summary.ms_level,
        polarity: summary.polarity,
    }
}

fn build_spec_summary(spectra: &[Spectrum]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(spectra.len() * SPEC_SUMMARY_SIZE);
    for spectrum in spectra {
        spec_summary_from_spectrum(spectrum).write_into(&mut buf);
    }
    buf
}

#[inline]
fn cv_f64(cv: &CvParam) -> Option<f64> {
    cv.value.as_deref().and_then(|s| s.parse::<f64>().ok())
}

fn extract_chrom_summary(chrom: &Chromatogram) -> ChromatogramSummary {
    let mut summary = ChromatogramSummary::unknown();
    for cv in &chrom.cv_params {
        match parse_accession_tail_raw(cv.accession.as_deref()) {
            LOWEST_OBSERVED_MZ => {
                if let Some(v) = cv_f64(cv) {
                    summary.lowest_mz = v;
                }
            }
            HIGHEST_OBSERVED_MZ => {
                if let Some(v) = cv_f64(cv) {
                    summary.highest_mz = v;
                }
            }
            LOWEST_OBSERVED_WAVELENGTH => {
                if let Some(v) = cv_f64(cv) {
                    summary.lowest_wavelength = v;
                }
            }
            HIGHEST_OBSERVED_WAVELENGTH => {
                if let Some(v) = cv_f64(cv) {
                    summary.highest_wavelength = v;
                }
            }
            LOWEST_ION_MOBILITY => {
                if let Some(v) = cv_f64(cv) {
                    summary.lowest_ion_mobility = v;
                }
            }
            HIGHEST_ION_MOBILITY => {
                if let Some(v) = cv_f64(cv) {
                    summary.highest_ion_mobility = v;
                }
            }
            POSITIVE_SCAN => summary.polarity = POLARITY_POSITIVE,
            NEGATIVE_SCAN => summary.polarity = POLARITY_NEGATIVE,
            _ => {}
        }
    }
    summary
}

fn build_chrom_summary(chroms: &[Chromatogram]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(chroms.len() * CHROM_SUMMARY_SIZE);
    for chrom in chroms {
        extract_chrom_summary(chrom).write_into(&mut buf);
    }
    buf
}

pub struct Encoder<'o> {
    output: &'o mut dyn EncoderOutput,
    config: EncodingConfig,
}

impl<'o> Encoder<'o> {
    pub fn new(output: &'o mut dyn EncoderOutput, config: EncodingConfig) -> Self {
        Self { output, config }
    }

    pub fn encode(&mut self, mzml: &MzML) -> IonResult<()> {
        let spectra = Self::spectra(mzml);
        let chroms = Self::chromatograms(mzml);
        let mut collector = MetaCollector::new();

        let spec_list_id = if mzml.run.spectrum_list.is_some() {
            collector.alloc()
        } else {
            0
        };
        let chrom_list_id = if mzml.run.chromatogram_list.is_some() {
            collector.alloc()
        } else {
            0
        };

        let spec_policy = self.config.array_policy(MZ_ARRAY);
        let chrom_policy = self.config.array_policy(TIME_ARRAY);

        let spectrum_meta = collector.collect_item_list_meta(
            spectra,
            spec_list_id,
            mzml.run.spectrum_list.as_ref(),
            spec_policy,
        );
        let chrom_meta = collector.collect_item_list_meta(
            chroms,
            chrom_list_id,
            mzml.run.chromatogram_list.as_ref(),
            chrom_policy,
        );
        let (global_meta, global_counts) = collector.collect_global_meta(mzml);

        let compressed = CompressedMetaSections::build(
            &spectrum_meta,
            &chrom_meta,
            &global_meta,
            &global_counts,
            self.config.compression_level,
        );

        let spec_meta_crc32 = crc32fast::hash(&compressed.spectrum_bytes);
        let chrom_meta_crc32 = crc32fast::hash(&compressed.chromatogram_bytes);
        let global_meta_crc32 = crc32fast::hash(&compressed.global_bytes);

        let spec_summary = build_spec_summary(spectra);
        let chrom_summary_bytes = build_chrom_summary(chroms);

        self.output.write_bytes(&[0u8; HEADER_SIZE])?;

        let (spec_arrays, chrom_arrays) = match self.config.writing_mode {
            WritingMode::Streaming => (
                pack_arrays_streaming(spectra, self.config, spec_policy, self.output)?,
                pack_arrays_streaming(chroms, self.config, chrom_policy, self.output)?,
            ),
            WritingMode::Memory => (
                pack_arrays_into_memory(spectra, self.config, spec_policy, self.output)?,
                pack_arrays_into_memory(chroms, self.config, chrom_policy, self.output)?,
            ),
        };

        let offsets = self.write_all_sections(
            &spec_summary,
            &spec_arrays,
            &chrom_summary_bytes,
            &chrom_arrays,
            &compressed,
        )?;

        self.output.write_bytes(&FILE_TRAILER)?;
        let total_file_size = self.output.current_byte_position()?;

        let header = Self::build_header(
            &self.config,
            &offsets,
            &spec_arrays,
            &chrom_arrays,
            &spectrum_meta,
            &chrom_meta,
            &global_meta,
            &compressed,
            &HeaderInfo {
                spectrum_count: spectra.len() as u64,
                chrom_count: chroms.len() as u64,
                total_file_size,
                spec_meta_crc32,
                chrom_meta_crc32,
                global_meta_crc32,
                off_spec_summary: offsets.offset_spec_summary,
                len_spec_summary: spec_summary.len() as u64,
                off_chrom_summary: offsets.offset_chrom_summary,
                len_chrom_summary: chrom_summary_bytes.len() as u64,
            },
        );

        let mut header_bytes = [0u8; HEADER_SIZE];
        header.write_into(&mut header_bytes);

        let header_crc = crc32fast::hash(&header_bytes[0..1020]);
        header_bytes[1020..1024].copy_from_slice(&header_crc.to_le_bytes());

        self.output.patch_bytes_at(0, &header_bytes)
    }

    fn spectra(mzml: &MzML) -> &[Spectrum] {
        mzml.run
            .spectrum_list
            .as_ref()
            .map_or(&[], |sl| &sl.spectra)
    }

    fn chromatograms(mzml: &MzML) -> &[Chromatogram] {
        mzml.run
            .chromatogram_list
            .as_ref()
            .map_or(&[], |cl| &cl.chromatograms)
    }

    fn write_all_sections(
        &mut self,
        spec_summary: &[u8],
        s: &PackedArraySection,
        chrom_summary: &[u8],
        c: &PackedArraySection,
        m: &CompressedMetaSections,
    ) -> IonResult<SectionOffsets> {
        Ok(SectionOffsets {
            offset_spec_summary: write_aligned_section(self.output, spec_summary)?,
            offset_spec_entries: write_aligned_section(self.output, &s.index_entries_bytes)?,
            offset_spec_arrayrefs: write_aligned_section(self.output, &s.array_refs_bytes)?,
            offset_chrom_summary: write_aligned_section(self.output, chrom_summary)?,
            offset_chrom_entries: write_aligned_section(self.output, &c.index_entries_bytes)?,
            offset_chrom_arrayrefs: write_aligned_section(self.output, &c.array_refs_bytes)?,
            offset_spec_meta: write_aligned_section(self.output, &m.spectrum_bytes)?,
            offset_chrom_meta: write_aligned_section(self.output, &m.chromatogram_bytes)?,
            offset_global_meta: write_aligned_section(self.output, &m.global_bytes)?,
            offset_packed_spectra: s.container_offset,
            offset_packed_chroms: c.container_offset,
        })
    }

    fn build_header(
        config: &EncodingConfig,
        offsets: &SectionOffsets,
        spec_arrays: &PackedArraySection,
        chrom_arrays: &PackedArraySection,
        spectrum_meta: &PackedMeta,
        chrom_meta: &PackedMeta,
        global_meta: &PackedMeta,
        compressed: &CompressedMetaSections,
        info: &HeaderInfo,
    ) -> FileHeader {
        FileHeader {
            compression_codec: config.codec_id(),
            compression_level: config.compression_level,
            array_filter_id: config.array_filter_id(),
            target_block_size: config.uncompressed_block_size as u64,

            offset_spec_entries: offsets.offset_spec_entries,
            len_spec_entries: spec_arrays.index_entries_bytes.len() as u64,
            offset_spec_arrayrefs: offsets.offset_spec_arrayrefs,
            len_spec_arrayrefs: spec_arrays.array_refs_bytes.len() as u64,
            offset_chrom_entries: offsets.offset_chrom_entries,
            len_chrom_entries: chrom_arrays.index_entries_bytes.len() as u64,
            offset_chrom_arrayrefs: offsets.offset_chrom_arrayrefs,
            len_chrom_arrayrefs: chrom_arrays.array_refs_bytes.len() as u64,
            offset_spec_meta: offsets.offset_spec_meta,
            len_spec_meta: compressed.spectrum_bytes.len() as u64,
            offset_chrom_meta: offsets.offset_chrom_meta,
            len_chrom_meta: compressed.chromatogram_bytes.len() as u64,
            offset_global_meta: offsets.offset_global_meta,
            len_global_meta: compressed.global_bytes.len() as u64,
            offset_packed_spectra: offsets.offset_packed_spectra,
            len_packed_spectra: spec_arrays.container_total_bytes,
            offset_packed_chroms: offsets.offset_packed_chroms,
            len_packed_chroms: chrom_arrays.container_total_bytes,

            spectrum_block_count: spec_arrays.block_count,
            chrom_block_count: chrom_arrays.block_count,
            spectrum_count: info.spectrum_count,
            chrom_count: info.chrom_count,

            spec_meta_row_count: spectrum_meta.ref_codes.len() as u64,
            spec_meta_numeric_count: spectrum_meta.numeric_values.len() as u64,
            spec_meta_string_count: spectrum_meta.string_offsets.len() as u64,
            chrom_meta_row_count: chrom_meta.ref_codes.len() as u64,
            chrom_meta_numeric_count: chrom_meta.numeric_values.len() as u64,
            chrom_meta_string_count: chrom_meta.string_offsets.len() as u64,
            global_meta_row_count: global_meta.ref_codes.len() as u64,
            global_meta_numeric_count: global_meta.numeric_values.len() as u64,
            global_meta_string_count: global_meta.string_offsets.len() as u64,
            spec_array_type_count: spec_arrays.seen_array_type_accessions.len() as u64,
            chrom_array_type_count: chrom_arrays.seen_array_type_accessions.len() as u64,

            spec_meta_uncompressed_size: compressed.spectrum_uncompressed_size,
            chrom_meta_uncompressed_size: compressed.chromatogram_uncompressed_size,
            global_meta_uncompressed_size: compressed.global_uncompressed_size,

            off_spec_summary: info.off_spec_summary,
            len_spec_summary: info.len_spec_summary,
            off_chrom_summary: info.off_chrom_summary,
            len_chrom_summary: info.len_chrom_summary,

            total_file_size: info.total_file_size,

            spec_meta_crc32: info.spec_meta_crc32,
            chrom_meta_crc32: info.chrom_meta_crc32,
            global_meta_crc32: info.global_meta_crc32,
            header_crc32: 0,
        }
    }
}

pub fn encode(
    mzml: &MzML,
    compression_level: u8,
    force_f32: bool,
    writing_mode: WritingMode,
    output: &mut dyn EncoderOutput,
) -> IonResult<()> {
    if compression_level > 22 {
        return Err(format!("compression_level must be 0–22, got {compression_level}").into());
    }
    let config = EncodingConfig {
        compression_level,
        force_f32,
        writing_mode,
        uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
        parallel: true,
    };
    Encoder::new(output, config).encode(mzml)
}

#[derive(Debug, Clone, Copy)]
pub struct EncodingConfig {
    pub compression_level: u8,
    pub force_f32: bool,
    pub writing_mode: WritingMode,
    pub uncompressed_block_size: usize,
    pub parallel: bool,
}

impl EncodingConfig {
    fn compression_is_enabled(self) -> bool {
        self.compression_level != 0
    }

    fn codec_id(self) -> u8 {
        self.compression_is_enabled() as u8
    }

    fn array_filter_id(self) -> u8 {
        if self.compression_is_enabled() {
            PackingId::ByteShuffle as u8
        } else {
            PackingId::Raw as u8
        }
    }

    fn compression_mode(self) -> CompressionMode<DefaultCompressor> {
        if self.compression_is_enabled() {
            CompressionMode::Compressed(
                DefaultCompressor::new(self.compression_level as i32).unwrap(),
            )
        } else {
            CompressionMode::Raw
        }
    }

    fn block_packing_id(self) -> PackingId {
        if self.compression_is_enabled() {
            PackingId::ByteShuffle
        } else {
            PackingId::Raw
        }
    }

    fn array_policy(self, x_array_accession: u32) -> ArrayPolicy {
        ArrayPolicy {
            x_array_accession,
            y_array_accession: INTENSITY_ARRAY,
            force_f32: self.force_f32,
        }
    }
}

struct SectionOffsets {
    offset_spec_summary: u64,
    offset_spec_entries: u64,
    offset_spec_arrayrefs: u64,
    offset_chrom_summary: u64,
    offset_chrom_entries: u64,
    offset_chrom_arrayrefs: u64,
    offset_spec_meta: u64,
    offset_chrom_meta: u64,
    offset_global_meta: u64,
    offset_packed_spectra: u64,
    offset_packed_chroms: u64,
}

struct HeaderInfo {
    spectrum_count: u64,
    chrom_count: u64,
    total_file_size: u64,
    spec_meta_crc32: u32,
    chrom_meta_crc32: u32,
    global_meta_crc32: u32,
    off_spec_summary: u64,
    len_spec_summary: u64,
    off_chrom_summary: u64,
    len_chrom_summary: u64,
}

#[derive(Copy, Clone)]
enum ArrayData<'a> {
    F16(&'a [u16]),
    F32(&'a [f32]),
    F64(&'a [f64]),
    I16(&'a [i16]),
    I32(&'a [i32]),
    I64(&'a [i64]),
}

impl<'a> ArrayData<'a> {
    fn element_count(self) -> usize {
        match self {
            Self::F16(e) => e.len(),
            Self::F32(e) => e.len(),
            Self::F64(e) => e.len(),
            Self::I16(e) => e.len(),
            Self::I32(e) => e.len(),
            Self::I64(e) => e.len(),
        }
    }

    fn is_empty(self) -> bool {
        self.element_count() == 0
    }
}

fn array_data_from_binary_data_array(bda: &BinaryDataArray) -> Option<ArrayData<'_>> {
    match bda.binary.as_ref()? {
        BinaryData::F16(e) => Some(ArrayData::F16(e)),
        BinaryData::I16(e) => Some(ArrayData::I16(e)),
        BinaryData::I32(e) => Some(ArrayData::I32(e)),
        BinaryData::I64(e) => Some(ArrayData::I64(e)),
        BinaryData::F32(e) => Some(ArrayData::F32(e)),
        BinaryData::F64(e) => Some(ArrayData::F64(e)),
    }
}

fn element_byte_size_for_dtype(dtype: u8) -> usize {
    match dtype {
        FILE_DTYPE_F16 | FILE_DTYPE_I16 => 2,
        FILE_DTYPE_F32 | FILE_DTYPE_I32 => 4,
        FILE_DTYPE_F64 | FILE_DTYPE_I64 => 8,
        _ => 1,
    }
}

fn resolve_array_dtype(bda: &BinaryDataArray, data: ArrayData<'_>, force_f32: bool) -> u8 {
    match data {
        ArrayData::F16(_) => FILE_DTYPE_F16,
        ArrayData::I16(_) => FILE_DTYPE_I16,
        ArrayData::I32(_) => FILE_DTYPE_I32,
        ArrayData::I64(_) => FILE_DTYPE_I64,
        ArrayData::F32(_) | ArrayData::F64(_) => {
            if float_data_should_be_written_as_f64(bda, data, force_f32) {
                FILE_DTYPE_F64
            } else {
                FILE_DTYPE_F32
            }
        }
    }
}

fn float_data_should_be_written_as_f64(
    bda: &BinaryDataArray,
    data: ArrayData<'_>,
    force_f32: bool,
) -> bool {
    if force_f32 {
        return false;
    }
    declared_float_precision_is_64bit(bda).unwrap_or(matches!(data, ArrayData::F64(_)))
}

fn declared_float_precision_is_64bit(bda: &BinaryDataArray) -> Option<bool> {
    if let Some(nt) = bda.numeric_type.as_ref() {
        return match nt {
            NumericType::Float64 => Some(true),
            NumericType::Float32 => Some(false),
            _ => None,
        };
    }
    let (mut saw32, mut saw64) = (false, false);
    for cv in &bda.cv_params {
        match parse_accession_tail_raw(cv.accession.as_deref()) {
            FLOAT_32BIT => saw32 = true,
            FLOAT_64BIT => saw64 = true,
            _ => {}
        }
        if saw32 && saw64 {
            break;
        }
    }
    match (saw32, saw64) {
        (true, false) => Some(false),
        (false, true) => Some(true),
        _ => None,
    }
}

fn validate_array_dtype(data: ArrayData<'_>, dtype: u8) -> IonResult<()> {
    let ok = matches!(
        (dtype, data),
        (FILE_DTYPE_F16, ArrayData::F16(_))
            | (FILE_DTYPE_F32, ArrayData::F32(_))
            | (FILE_DTYPE_F32, ArrayData::F64(_))
            | (FILE_DTYPE_F64, ArrayData::F64(_))
            | (FILE_DTYPE_F64, ArrayData::F32(_))
            | (FILE_DTYPE_I16, ArrayData::I16(_))
            | (FILE_DTYPE_I32, ArrayData::I32(_))
            | (FILE_DTYPE_I64, ArrayData::I64(_))
    );
    if ok {
        Ok(())
    } else {
        Err(format!(
            "write_array_data: incompatible dtype {dtype} for the given array data variant"
        )
        .into())
    }
}

fn write_array_data(buf: &mut Vec<u8>, data: ArrayData<'_>, dtype: u8) {
    match (dtype, data) {
        (FILE_DTYPE_F16, ArrayData::F16(e)) => write_u16_slice_le(buf, e),
        (FILE_DTYPE_F32, ArrayData::F32(e)) => write_f32_slice_le(buf, e),
        (FILE_DTYPE_F32, ArrayData::F64(e)) => {
            for &v in e {
                write_f32_le(buf, v as f32);
            }
        }
        (FILE_DTYPE_F64, ArrayData::F64(e)) => write_f64_slice_le(buf, e),
        (FILE_DTYPE_F64, ArrayData::F32(e)) => {
            for &v in e {
                write_f64_le(buf, v as f64);
            }
        }
        (FILE_DTYPE_I16, ArrayData::I16(e)) => write_i16_slice_le(buf, e),
        (FILE_DTYPE_I32, ArrayData::I32(e)) => write_i32_slice_le(buf, e),
        (FILE_DTYPE_I64, ArrayData::I64(e)) => write_i64_slice_le(buf, e),
        // SAFETY: `validate_array_dtype` is always called before this function.
        _ => unreachable!("write_array_data called with unvalidated dtype/data combination"),
    }
}

fn write_aligned_section(output: &mut dyn EncoderOutput, bytes: &[u8]) -> IonResult<u64> {
    static PAD: [u8; 7] = [0u8; 7];
    let pos = output.current_byte_position()?;
    let aligned = (pos + 7) & !7;
    if aligned > pos {
        output.write_bytes(&PAD[..(aligned - pos) as usize])?;
    }
    output.write_bytes(bytes)?;
    Ok(aligned)
}

const INDEX_ENTRY_BYTES: usize = 16;
const ARRAYREF_ENTRY_BYTES: usize = 32;
const AVG_ARRAYS_PER_ITEM: usize = 2;

fn write_arrayref_entry(
    buf: &mut Vec<u8>,
    element_offset: u64,
    element_count: u64,
    block_id: u32,
    array_accession: u32,
    dtype: u8,
    array_filter: u8,
    encoded_len: u32,
) {
    write_u64_le(buf, element_offset);
    write_u64_le(buf, element_count);
    write_u32_le(buf, block_id);
    write_u32_le(buf, array_accession);
    buf.push(dtype);
    buf.push(array_filter);
    write_u32_le(buf, encoded_len);
    buf.extend_from_slice(&[0u8; 2]);
}

struct PackedArraySection {
    block_count: u64,
    container_offset: u64,
    container_total_bytes: u64,
    index_entries_bytes: Vec<u8>,
    array_refs_bytes: Vec<u8>,
    seen_array_type_accessions: Vec<u32>,
}

trait HasBinaryDataArrayList {
    fn binary_data_array_list(&self) -> Option<&BinaryDataArrayList>;
}

impl HasBinaryDataArrayList for Spectrum {
    fn binary_data_array_list(&self) -> Option<&BinaryDataArrayList> {
        self.binary_data_array_list.as_ref()
    }
}

impl HasBinaryDataArrayList for Chromatogram {
    fn binary_data_array_list(&self) -> Option<&BinaryDataArrayList> {
        self.binary_data_array_list.as_ref()
    }
}

struct EncodedArrayRef {
    element_offset: u64,
    element_count: u64,
    block_id: u32,
    accession: u32,
    dtype: u8,
    array_filter: u8,
    encoded_len: u32,
}

fn encode_variable_length_array(
    data: ArrayData<'_>,
    dtype: u8,
    dtype_enum: Dtype,
    packing: &'static dyn Packing,
    container: &mut ContainerBuilder<'_, DefaultCompressor>,
) -> IonResult<(u32, u64, u32)> {
    let mut encoded = Vec::new();
    match (data, dtype_enum) {
        (ArrayData::F64(s), Dtype::F64) => packing.encode(PackingInput::F64(s), &mut encoded)?,
        (ArrayData::F32(s), Dtype::F32) => packing.encode(PackingInput::F32(s), &mut encoded)?,
        _ => write_array_data(&mut encoded, data, dtype),
    }
    let enc_len =
        u32::try_from(encoded.len()).map_err(|_| IonError::from("encoded array exceeds 4 GiB"))?;
    let (bid, eoff) = container.add_item_to_box(encoded.len(), 1, |buf| {
        buf.extend_from_slice(&encoded);
        Ok(())
    })?;
    Ok((bid, eoff, enc_len))
}

fn encode_fixed_length_array(
    data: ArrayData<'_>,
    dtype: u8,
    dtype_enum: Dtype,
    elem_bytes: usize,
    packing: &'static dyn Packing,
    container: &mut ContainerBuilder<'_, DefaultCompressor>,
) -> IonResult<(u32, u64, u32)> {
    let (bid, eoff) =
        container.add_item_to_box(data.element_count() * elem_bytes, elem_bytes, |buf| {
            match packing.id() {
                PackingId::DeltaShuffle => match (data, dtype_enum) {
                    (ArrayData::F64(slice), Dtype::F64) => {
                        packing.encode(PackingInput::F64(slice), buf)
                    }
                    _ => {
                        write_array_data(buf, data, dtype);
                        Ok(())
                    }
                },
                _ => {
                    write_array_data(buf, data, dtype);
                    Ok(())
                }
            }
        })?;
    Ok((bid, eoff, 0u32))
}

fn encode_single_array(
    bda: &BinaryDataArray,
    config: EncodingConfig,
    policy: ArrayPolicy,
    container: &mut ContainerBuilder<'_, DefaultCompressor>,
) -> IonResult<Option<EncodedArrayRef>> {
    let Some(data) = array_data_from_binary_data_array(bda) else {
        return Ok(None);
    };
    if data.is_empty() {
        return Ok(None);
    }
    let accession = array_type_accession_from_binary_data_array(bda);
    let dtype = resolve_array_dtype(bda, data, policy.should_force_f32(accession));
    validate_array_dtype(data, dtype)?;
    let elem_bytes = element_byte_size_for_dtype(dtype);
    let dtype_enum = Dtype::from_byte(dtype).unwrap_or(Dtype::F64);
    let requested: &'static dyn Packing = if config.compression_is_enabled() {
        packing_for(accession, dtype_enum, data.element_count())
    } else {
        &RAW_PACKING
    };
    let packing: &'static dyn Packing = if requested.supports(dtype_enum) {
        requested
    } else {
        &RAW_PACKING
    };
    let (block_id, element_offset, encoded_len) = if packing.is_variable_length() {
        encode_variable_length_array(data, dtype, dtype_enum, packing, container)?
    } else {
        encode_fixed_length_array(data, dtype, dtype_enum, elem_bytes, packing, container)?
    };
    Ok(Some(EncodedArrayRef {
        element_offset,
        element_count: data.element_count() as u64,
        block_id,
        accession,
        dtype,
        array_filter: packing.id() as u8,
        encoded_len,
    }))
}

fn fill_container<T: HasBinaryDataArrayList>(
    items: &[T],
    config: EncodingConfig,
    policy: ArrayPolicy,
    container: &mut ContainerBuilder<'_, DefaultCompressor>,
    index_bytes: &mut Vec<u8>,
    aref_bytes: &mut Vec<u8>,
    seen_types: &mut Vec<u32>,
) -> IonResult<()> {
    let mut aref_cursor: u64 = 0;
    for item in items {
        let aref_start = aref_cursor;
        let mut aref_count: u64 = 0;
        if let Some(list) = item.binary_data_array_list() {
            for bda in &list.binary_data_arrays {
                let Some(aref) = encode_single_array(bda, config, policy, container)? else {
                    continue;
                };
                if aref.accession != 0 && !seen_types.contains(&aref.accession) {
                    seen_types.push(aref.accession);
                }
                write_arrayref_entry(
                    aref_bytes,
                    aref.element_offset,
                    aref.element_count,
                    aref.block_id,
                    aref.accession,
                    aref.dtype,
                    aref.array_filter,
                    aref.encoded_len,
                );
                aref_cursor += 1;
                aref_count += 1;
            }
        }
        write_u64_le(index_bytes, aref_start);
        write_u64_le(index_bytes, aref_count);
    }
    Ok(())
}

fn pack_arrays_into_memory<T: HasBinaryDataArrayList>(
    items: &[T],
    config: EncodingConfig,
    policy: ArrayPolicy,
    output: &mut dyn EncoderOutput,
) -> IonResult<PackedArraySection> {
    let mut container_bytes = Vec::new();
    let mut index_bytes = Vec::with_capacity(items.len() * INDEX_ENTRY_BYTES);
    let mut aref_bytes =
        Vec::with_capacity(items.len() * ARRAYREF_ENTRY_BYTES * AVG_ARRAYS_PER_ITEM);
    let mut seen_types = Vec::with_capacity(8);
    let builder = ContainerBuilder::new(
        &mut container_bytes,
        config.uncompressed_block_size,
        config.compression_mode(),
        config.block_packing_id(),
    );
    let mut container = if config.parallel {
        builder
    } else {
        builder.force_sequential()
    };
    fill_container(
        items,
        config,
        policy,
        &mut container,
        &mut index_bytes,
        &mut aref_bytes,
        &mut seen_types,
    )?;
    let (block_count, container_total_bytes) = container.finish()?;
    let container_offset = write_aligned_section(output, &container_bytes)?;
    Ok(PackedArraySection {
        block_count: block_count as u64,
        container_offset,
        container_total_bytes,
        index_entries_bytes: index_bytes,
        array_refs_bytes: aref_bytes,
        seen_array_type_accessions: seen_types,
    })
}

fn pack_arrays_streaming<T: HasBinaryDataArrayList>(
    items: &[T],
    config: EncodingConfig,
    policy: ArrayPolicy,
    output: &mut dyn EncoderOutput,
) -> IonResult<PackedArraySection> {
    let mut index_bytes = Vec::with_capacity(items.len() * INDEX_ENTRY_BYTES);
    let mut aref_bytes =
        Vec::with_capacity(items.len() * ARRAYREF_ENTRY_BYTES * AVG_ARRAYS_PER_ITEM);
    let mut seen_types = Vec::with_capacity(8);
    let container_offset = write_aligned_section(output, &[])?;
    let builder = ContainerBuilder::new(
        output,
        config.uncompressed_block_size,
        config.compression_mode(),
        config.block_packing_id(),
    );
    let mut container = if config.parallel {
        builder
    } else {
        builder.force_sequential()
    };
    fill_container(
        items,
        config,
        policy,
        &mut container,
        &mut index_bytes,
        &mut aref_bytes,
        &mut seen_types,
    )?;
    let (block_count, container_total_bytes) = container.finish()?;
    Ok(PackedArraySection {
        block_count: block_count as u64,
        container_offset,
        container_total_bytes,
        index_entries_bytes: index_bytes,
        array_refs_bytes: aref_bytes,
        seen_array_type_accessions: seen_types,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ion::{
        format::FILE_SIGNATURE,
        utilities::parse_header::{
            HEADER_CHROM_BLOCK_COUNT, HEADER_SPECTRUM_BLOCK_COUNT, HEADER_TARGET_BLOCK_SIZE,
        },
    };

    #[test]
    fn encoder_struct_and_free_fn_are_equivalent() {
        let mzml = MzML::default();
        let mut via_struct = Vec::new();
        let mut via_free_fn = Vec::new();

        Encoder::new(
            &mut via_struct,
            EncodingConfig {
                compression_level: 0,
                force_f32: false,
                writing_mode: WritingMode::Memory,
                uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
                parallel: true,
            },
        )
        .encode(&mzml)
        .unwrap();

        encode(&mzml, 0, false, WritingMode::Memory, &mut via_free_fn).unwrap();

        assert_eq!(via_struct, via_free_fn);
    }

    #[test]
    fn memory_and_streaming_modes_produce_identical_output() {
        let mzml = MzML::default();
        let mut memory_output = Vec::new();
        let mut streaming_output = Vec::new();

        encode(&mzml, 0, false, WritingMode::Memory, &mut memory_output).unwrap();
        encode(
            &mzml,
            0,
            false,
            WritingMode::Streaming,
            &mut streaming_output,
        )
        .unwrap();

        assert_eq!(memory_output, streaming_output);
    }

    #[test]
    fn encoder_output_starts_with_magic_and_ends_with_trailer() {
        let mzml = MzML::default();
        let mut buf = Vec::new();
        Encoder::new(
            &mut buf,
            EncodingConfig {
                compression_level: 0,
                force_f32: false,
                writing_mode: WritingMode::Streaming,
                uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
                parallel: true,
            },
        )
        .encode(&mzml)
        .unwrap();

        assert!(buf.len() >= HEADER_SIZE + FILE_TRAILER.len());
        assert_eq!(&buf[0..FILE_SIGNATURE.len()], &FILE_SIGNATURE);
        assert_eq!(&buf[buf.len() - FILE_TRAILER.len()..], &FILE_TRAILER);
    }

    #[test]
    fn encoder_output_total_file_size_is_correct() {
        let mzml = MzML::default();
        let mut buf = Vec::new();
        encode(&mzml, 0, false, WritingMode::Memory, &mut buf).unwrap();

        let total_file_size = u64::from_le_bytes(
            buf[crate::ion::utilities::parse_header::HEADER_TOTAL_FILE_SIZE
                ..crate::ion::utilities::parse_header::HEADER_TOTAL_FILE_SIZE + 8]
                .try_into()
                .unwrap(),
        );
        assert_eq!(total_file_size, buf.len() as u64);
    }

    #[test]
    fn encoder_header_target_block_size_matches_default() {
        let mzml = MzML::default();
        let mut buf = Vec::new();
        encode(&mzml, 0, false, WritingMode::Memory, &mut buf).unwrap();

        let target_block_size = u64::from_le_bytes(
            buf[HEADER_TARGET_BLOCK_SIZE..HEADER_TARGET_BLOCK_SIZE + 8]
                .try_into()
                .unwrap(),
        );
        assert_eq!(target_block_size, TARGET_BLOCK_UNCOMPRESSED_BYTES as u64);
    }

    #[test]
    fn encoder_header_crc32_is_valid() {
        let mzml = MzML::default();
        let mut buf = Vec::new();
        encode(&mzml, 0, false, WritingMode::Memory, &mut buf).unwrap();

        let stored_crc = u32::from_le_bytes(buf[1020..1024].try_into().unwrap());
        let computed_crc = crc32fast::hash(&buf[0..1020]);
        assert_eq!(stored_crc, computed_crc);
    }

    #[test]
    fn spec_summary_size_is_correct() {
        let mut buf = Vec::new();
        SpectrumSummary::unknown().write_into(&mut buf);
        assert_eq!(buf.len(), SPEC_SUMMARY_SIZE);
    }

    #[test]
    fn spec_summary_unknown_summary_is_all_nan_and_zero() {
        let mut buf = Vec::new();
        SpectrumSummary::unknown().write_into(&mut buf);
        for i in (0..40).step_by(8) {
            let v = f64::from_le_bytes(buf[i..i + 8].try_into().unwrap());
            assert!(v.is_nan());
        }
        assert_eq!(buf[40], 0);
        assert_eq!(buf[41], 0);
    }

    #[test]
    fn spec_summary_index_len_matches_spectrum_count() {
        let mzml = MzML::default();
        let spectra = Encoder::spectra(&mzml);
        let index = build_spec_summary(spectra);
        assert_eq!(index.len(), spectra.len() * SPEC_SUMMARY_SIZE);
    }

    #[test]
    fn chrom_summary_size_is_correct() {
        let mut buf = Vec::new();
        ChromatogramSummary::unknown().write_into(&mut buf);
        assert_eq!(buf.len(), CHROM_SUMMARY_SIZE);
    }

    #[test]
    fn chrom_summary_unknown_summary_is_all_nan_and_zero() {
        let mut buf = Vec::new();
        ChromatogramSummary::unknown().write_into(&mut buf);
        for i in (0..48).step_by(8) {
            let v = f64::from_le_bytes(buf[i..i + 8].try_into().unwrap());
            assert!(v.is_nan());
        }
        assert_eq!(buf[48], 0);
    }

    #[test]
    fn chrom_summary_index_len_matches_chrom_count() {
        let mzml = MzML::default();
        let chroms = Encoder::chromatograms(&mzml);
        let index = build_chrom_summary(chroms);
        assert_eq!(index.len(), chroms.len() * CHROM_SUMMARY_SIZE);
    }

    #[test]
    fn chrom_summary_extracts_cv_values() {
        use crate::mzml::structs::{Chromatogram, CvParam};

        fn cv(accession: &str, value: &str) -> CvParam {
            CvParam {
                accession: Some(accession.to_string()),
                value: Some(value.to_string()),
                ..Default::default()
            }
        }

        let chrom = Chromatogram {
            cv_params: vec![
                cv("MS:1000528", "100.5"),
                cv("MS:1000527", "999.9"),
                cv("MS:1000619", "200.0"),
                cv("MS:1000618", "800.0"),
                cv("MS:1003437", "0.5"),
                cv("MS:1003438", "2.5"),
                CvParam {
                    accession: Some("MS:1000130".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let summary = extract_chrom_summary(&chrom);
        assert_eq!(summary.lowest_mz, 100.5);
        assert_eq!(summary.highest_mz, 999.9);
        assert_eq!(summary.lowest_wavelength, 200.0);
        assert_eq!(summary.highest_wavelength, 800.0);
        assert_eq!(summary.lowest_ion_mobility, 0.5);
        assert_eq!(summary.highest_ion_mobility, 2.5);
        assert_eq!(summary.polarity, 1);
    }

    #[test]
    fn encoder_config_policies_have_correct_accessions() {
        let config = EncodingConfig {
            compression_level: 3,
            force_f32: true,
            writing_mode: WritingMode::Streaming,
            uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
            parallel: true,
        };
        let sp = config.array_policy(MZ_ARRAY);
        assert_eq!(sp.x_array_accession, MZ_ARRAY);
        assert_eq!(sp.y_array_accession, INTENSITY_ARRAY);
        assert!(sp.force_f32);

        let cp = config.array_policy(TIME_ARRAY);
        assert_eq!(cp.x_array_accession, TIME_ARRAY);
        assert_eq!(cp.y_array_accession, INTENSITY_ARRAY);
    }

    #[test]
    fn encoder_config_compression_disabled_at_level_zero() {
        let config = EncodingConfig {
            compression_level: 0,
            force_f32: false,
            writing_mode: WritingMode::Streaming,
            uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
            parallel: true,
        };
        assert!(!config.compression_is_enabled());
        assert_eq!(config.codec_id(), 0);
        assert_eq!(config.array_filter_id(), PackingId::Raw as u8);
        assert!(matches!(config.block_packing_id(), PackingId::Raw));
    }

    #[test]
    fn encoder_config_compression_enabled_at_nonzero_level() {
        let config = EncodingConfig {
            compression_level: 3,
            force_f32: false,
            writing_mode: WritingMode::Streaming,
            uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
            parallel: true,
        };
        assert!(config.compression_is_enabled());
        assert_eq!(config.codec_id(), 1);
        assert_eq!(config.array_filter_id(), PackingId::ByteShuffle as u8);
        assert!(matches!(config.block_packing_id(), PackingId::ByteShuffle));
    }

    #[test]
    fn encoder_header_block_counts_at_correct_offsets() {
        let mzml = MzML::default();
        let mut buf = Vec::new();
        Encoder::new(
            &mut buf,
            EncodingConfig {
                compression_level: 0,
                force_f32: false,
                writing_mode: WritingMode::Streaming,
                uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
                parallel: true,
            },
        )
        .encode(&mzml)
        .unwrap();

        let spec_blocks = u64::from_le_bytes(
            buf[HEADER_SPECTRUM_BLOCK_COUNT..HEADER_SPECTRUM_BLOCK_COUNT + 8]
                .try_into()
                .unwrap(),
        );
        let chrom_blocks = u64::from_le_bytes(
            buf[HEADER_CHROM_BLOCK_COUNT..HEADER_CHROM_BLOCK_COUNT + 8]
                .try_into()
                .unwrap(),
        );
        assert_eq!(spec_blocks, 0);
        assert_eq!(chrom_blocks, 0);
    }

    #[test]
    fn write_aligned_section_pads_to_8_bytes() {
        let mut output: Vec<u8> = vec![0u8; 3];
        let start = write_aligned_section(&mut output, &[0xAAu8; 4]).unwrap();
        assert_eq!(start, 8);
        assert_eq!(output.len(), 12);
        assert_eq!(&output[3..8], &[0u8; 5]);
        assert_eq!(&output[8..12], &[0xAAu8; 4]);
    }

    #[test]
    fn write_aligned_section_no_padding_when_already_aligned() {
        let mut output: Vec<u8> = vec![0u8; 8];
        let start = write_aligned_section(&mut output, &[0xBBu8; 4]).unwrap();
        assert_eq!(start, 8);
        assert_eq!(output.len(), 12);
    }

    #[test]
    fn vec_encoder_output_patch_bytes_at() {
        let mut output = vec![0u8; 16];
        output.patch_bytes_at(4, &[1u8, 2, 3, 4]).unwrap();
        assert_eq!(&output[4..8], &[1u8, 2, 3, 4]);
    }

    #[test]
    fn vec_encoder_output_patch_out_of_bounds_errors() {
        let mut output = vec![0u8; 4];
        assert!(output.patch_bytes_at(3, &[1u8, 2, 3]).is_err());
    }

    #[test]
    fn file_header_starts_with_magic_bytes() {
        let mut buf = [0u8; HEADER_SIZE];
        FileHeader::default().write_into(&mut buf);
        assert_eq!(&buf[0..FILE_SIGNATURE.len()], &FILE_SIGNATURE);
    }

    #[test]
    fn file_header_block_counts_at_correct_offsets() {
        let mut buf = [0u8; HEADER_SIZE];
        FileHeader {
            spectrum_block_count: 7u64,
            chrom_block_count: 13u64,
            ..FileHeader::default()
        }
        .write_into(&mut buf);
        assert_eq!(
            u64::from_le_bytes(
                buf[HEADER_SPECTRUM_BLOCK_COUNT..HEADER_SPECTRUM_BLOCK_COUNT + 8]
                    .try_into()
                    .unwrap()
            ),
            7u64
        );
        assert_eq!(
            u64::from_le_bytes(
                buf[HEADER_CHROM_BLOCK_COUNT..HEADER_CHROM_BLOCK_COUNT + 8]
                    .try_into()
                    .unwrap()
            ),
            13u64
        );
    }

    #[test]
    fn declared_float_precision_prefers_numeric_type_field() {
        let mut bda = BinaryDataArray::default();
        bda.numeric_type = Some(NumericType::Float64);
        assert_eq!(declared_float_precision_is_64bit(&bda), Some(true));
        bda.numeric_type = Some(NumericType::Float32);
        assert_eq!(declared_float_precision_is_64bit(&bda), Some(false));
    }

    #[test]
    fn resolve_array_dtype_force_f32_overrides_f64_data() {
        let bda = BinaryDataArray::default();
        assert_eq!(
            resolve_array_dtype(&bda, ArrayData::F64(&[1.0f64]), true),
            FILE_DTYPE_F32
        );
    }

    #[test]
    fn resolve_array_dtype_integer_types_unchanged_by_force_f32() {
        let bda = BinaryDataArray::default();
        assert_eq!(
            resolve_array_dtype(&bda, ArrayData::I32(&[1i32]), true),
            FILE_DTYPE_I32
        );
    }

    #[test]
    fn element_byte_size_for_dtype_returns_correct_sizes() {
        assert_eq!(element_byte_size_for_dtype(FILE_DTYPE_F64), 8);
        assert_eq!(element_byte_size_for_dtype(FILE_DTYPE_F32), 4);
        assert_eq!(element_byte_size_for_dtype(FILE_DTYPE_F16), 2);
        assert_eq!(element_byte_size_for_dtype(FILE_DTYPE_I16), 2);
        assert_eq!(element_byte_size_for_dtype(FILE_DTYPE_I32), 4);
        assert_eq!(element_byte_size_for_dtype(FILE_DTYPE_I64), 8);
    }
}
