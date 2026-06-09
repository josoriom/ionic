use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use std::path::Path;

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use rayon::prelude::*;

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use crate::ion::decoder::utilities::byte_source::MmapSource;

use crate::{
    accessions::format_accession,
    encoder::encode::{CHROM_SUMMARY_SIZE, SPEC_SUMMARY_SIZE},
    ion::{
        IonError, IonResult,
        attr_meta::{
            ACC_ATTR_DEFAULT_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_DEFAULT_SOURCE_FILE_REF,
            ACC_ATTR_ID, ACC_ATTR_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_REF, ACC_ATTR_SAMPLE_REF,
            ACC_ATTR_START_TIME_STAMP, AccessionTail, parse_accession_tail,
        },
        decoder::async_reader::AsyncReader,
        decoder::utilities::byte_source::{
            AsyncByteSource, ByteSource, Query, QueryCallbackSource, QueryPayload, QueryPromise,
            SliceSource,
        },
        decoder::utilities::common::decompress_zstd,
        encoder::encode::{
            FILE_DTYPE_F16, FILE_DTYPE_F32, FILE_DTYPE_F64, FILE_DTYPE_I16, FILE_DTYPE_I32,
            FILE_DTYPE_I64,
        },
        filter_summary::{ChromatogramSummary, SpectrumSummary},
        format::{CODEC_NONE, CODEC_ZSTD},
        meta_groups::MetaTotals,
        packing::PackingId,
        utilities::{
            MetaGroupReader,
            children_lookup::{ChildrenLookup, DefaultMetadataPolicy, OwnerRows},
            common::get_attr_text,
            container_view::{
                ContainerAccess, ContainerView, DefaultBlockProcessor, container_directory_range,
            },
            decompression_budget::DecompressionBudget,
            parse_chromatogram_list, parse_cv_and_user_params, parse_cv_list,
            parse_data_processing_list, parse_file_description,
            parse_global_metadata::parse_global_metadata,
            parse_header::{Header, parse_header},
            parse_instrument_list, parse_referenceable_param_group_list, parse_sample_list,
            parse_scan_settings_list, parse_software_list, parse_spectrum, parse_spectrum_list,
            segment_bounds::SegmentBoundsIndex,
            spectrum_source::{
                ScanSource, ScanSummary, f16_bits_to_f64, load_scan_from_spectra,
                summary_from_spectra, summary_from_spectrum,
            },
        },
    },
    mzml::{schema::TagId, structs::*},
};

const ACC_MZ: u32 = 1_000_514;
const ACC_INT: u32 = 1_000_515;
const INDEX_ENTRY_BYTES: usize = 16;
const ARRAY_REF_BYTES: usize = 32;
const DEFAULT_MAX_CACHED_BYTES: usize = 256 * 1024 * 1024;
const INLINE_ARRAY_REF_CAP: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub enum MetadatumValue {
    Number(f64),
    Text(String),
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metadatum {
    pub(crate) item_index: u32,
    pub(crate) id: u32,
    pub(crate) parent_id: u32,
    pub(crate) tag_id: TagId,
    pub(crate) accession: Option<String>,
    pub(crate) unit_accession: Option<String>,
    pub(crate) value: MetadatumValue,
}

#[inline]
pub(crate) fn slice_at<'a>(
    bytes: &'a [u8],
    off: u64,
    len: u64,
    context: &str,
) -> IonResult<&'a [u8]> {
    let end = off
        .checked_add(len)
        .ok_or_else(|| IonError::from(format!("{context}: range error")))?;
    let start =
        usize::try_from(off).map_err(|_| IonError::from(format!("{context}: range error")))?;
    let end =
        usize::try_from(end).map_err(|_| IonError::from(format!("{context}: range error")))?;
    bytes
        .get(start..end)
        .ok_or_else(|| IonError::from(format!("{context}: range error")))
}

pub(crate) fn open_byte_ranges(header: &Header) -> IonResult<Vec<(u64, u64)>> {
    let mut ranges = vec![
        (0, 1024),
        (header.off_spec_summary, header.len_spec_summary),
        (header.off_chrom_summary, header.len_chrom_summary),
        (header.off_spec_entries, header.len_spec_entries),
        (header.off_spec_arrayrefs, header.len_spec_arrayrefs),
        (header.off_chrom_entries, header.len_chrom_entries),
        (header.off_chrom_arrayrefs, header.len_chrom_arrayrefs),
        (header.off_global_meta, header.len_global_meta),
        (header.off_spec_meta, header.len_spec_meta),
        (header.off_chrom_meta, header.len_chrom_meta),
    ];

    let spec_blocks = usize::try_from(header.spec_block_count)
        .map_err(|_| IonError::from("spec: block count too large for this platform"))?;
    ranges.push(container_directory_range(
        header.off_spec_container,
        header.len_spec_container,
        spec_blocks,
        "spec",
    )?);

    if header.chrom_block_count > 0 && header.len_chrom_container > 0 {
        let chrom_blocks = usize::try_from(header.chrom_block_count)
            .map_err(|_| IonError::from("chrom: block count too large for this platform"))?;
        ranges.push(container_directory_range(
            header.off_chrom_container,
            header.len_chrom_container,
            chrom_blocks,
            "chrom",
        )?);
    }

    Ok(ranges)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ArrayRef {
    pub block_id: u32,
    pub element_offset: u64,
    pub element_count: u64,
    pub array_type: u32,
    pub dtype: u8,
    pub array_filter: u8,
    pub encoded_len: u32,
    pub continues_previous_segment: u8,
}

#[derive(Debug, Clone)]
pub struct ArrayGroup {
    pub array_type: u32,
    pub dtype: u8,
    pub array_filter: u8,
    pub refs: Vec<ArrayRef>,
}

#[derive(Debug, Clone)]
pub struct DecoderConfig {
    pub max_cached_bytes: usize,
    pub verify_checksums: bool,
    pub parallel: bool,
    pub decompression_budget: DecompressionBudget,
}

impl Default for DecoderConfig {
    fn default() -> Self {
        Self {
            max_cached_bytes: DEFAULT_MAX_CACHED_BYTES,
            verify_checksums: true,
            parallel: true,
            decompression_budget: DecompressionBudget::default(),
        }
    }
}

enum SegmentBoundsCache {
    Unloaded,
    Absent,
    Loaded(SegmentBoundsIndex),
}

pub struct Decoder {
    header: Header,
    source: Arc<dyn ByteSource>,
    spec_segment_bounds: SegmentBoundsCache,
    chrom_segment_bounds: SegmentBoundsCache,
    spec_summary_buf: Arc<[u8]>,
    chrom_summary_buf: Arc<[u8]>,
    spec_entries_buf: Arc<[u8]>,
    spec_array_refs: Arc<[u8]>,
    chrom_entries_buf: Arc<[u8]>,
    chrom_array_refs: Arc<[u8]>,
    global_meta_buf: Arc<[u8]>,
    spec_container: ContainerView<DefaultBlockProcessor>,
    chrom_container: Option<ContainerView<DefaultBlockProcessor>>,
    spec_meta_reader: MetaGroupReader,
    chrom_meta_reader: MetaGroupReader,
    mz_values: Vec<f64>,
    int_values: Vec<f64>,
    parallel: bool,
    decompression_budget: DecompressionBudget,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayWindow {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
}

impl ArrayWindow {
    fn empty() -> Self {
        Self {
            x: Vec::new(),
            y: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpectrumWindow {
    pub mz: Vec<f64>,
    pub intensity: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Spec,
    Chrom,
}

#[derive(Debug, Clone, Copy)]
pub struct ItemSlice {
    pub item_index: u64,
    pub array_ref_index: u64,
}

impl Decoder {
    pub fn open(bytes: &[u8], config: DecoderConfig) -> IonResult<Self> {
        let file_bytes: Arc<[u8]> = Arc::from(bytes);
        Self::open_arc(file_bytes, config)
    }

    pub fn open_arc(file_bytes: Arc<[u8]>, config: DecoderConfig) -> IonResult<Self> {
        let source = Arc::new(SliceSource::new(file_bytes)) as Arc<dyn ByteSource>;
        Self::open_with_source(source, config)
    }

    pub fn open_with_query(
        read: impl Fn(Query) -> IonResult<QueryPayload> + Send + Sync + 'static,
        config: DecoderConfig,
    ) -> IonResult<Self> {
        let source = Arc::new(QueryCallbackSource::new(read)) as Arc<dyn ByteSource>;
        Self::open_with_source(source, config)
    }

    pub fn open_with_source(source: Arc<dyn ByteSource>, config: DecoderConfig) -> IonResult<Self> {
        let header_buf = source.read(0, 1024)?;
        let header = parse_header(&header_buf)?;
        let block_packing_id = PackingId::from_byte(header.default_array_filter)?;

        let spec_summary_buf = source.read(header.off_spec_summary, header.len_spec_summary)?;
        let chrom_summary_buf = source.read(header.off_chrom_summary, header.len_chrom_summary)?;
        let spec_entries_buf = source.read(header.off_spec_entries, header.len_spec_entries)?;
        let spec_array_refs = source.read(header.off_spec_arrayrefs, header.len_spec_arrayrefs)?;
        let chrom_entries_buf = source.read(header.off_chrom_entries, header.len_chrom_entries)?;
        let chrom_array_refs =
            source.read(header.off_chrom_arrayrefs, header.len_chrom_arrayrefs)?;
        let global_meta_buf = source.read(header.off_global_meta, header.len_global_meta)?;

        let spec_container = ContainerView::new(
            source.clone(),
            header.off_spec_container,
            header.len_spec_container,
            header.spec_block_count,
            header.spec_directory_crc32,
            header.compression_level,
            block_packing_id,
            config.verify_checksums,
            "spec",
            DefaultBlockProcessor,
            config.max_cached_bytes,
            config.decompression_budget,
        )?;

        let chrom_container = if header.chrom_block_count > 0 && header.len_chrom_container > 0 {
            Some(ContainerView::new(
                source.clone(),
                header.off_chrom_container,
                header.len_chrom_container,
                header.chrom_block_count,
                header.chrom_directory_crc32,
                header.compression_level,
                block_packing_id,
                config.verify_checksums,
                "chrom",
                DefaultBlockProcessor,
                config.max_cached_bytes,
                config.decompression_budget,
            )?)
        } else {
            None
        };

        let spec_meta_reader = MetaGroupReader::new(
            Arc::from(source.read(header.off_spec_meta, header.len_spec_meta)?),
            header.spec_meta_group_count,
            header.meta_group_size,
            header.spectrum_count,
            MetaTotals {
                rows: header.spec_meta_count,
                numeric: header.spec_meta_numeric_count,
                string: header.spec_meta_string_count,
                uncompressed: header.spec_meta_uncompressed_bytes,
            },
            header.compression_codec,
            config.verify_checksums,
            config.decompression_budget,
            config.max_cached_bytes,
        )?;

        let chrom_meta_reader = MetaGroupReader::new(
            Arc::from(source.read(header.off_chrom_meta, header.len_chrom_meta)?),
            header.chrom_meta_group_count,
            header.meta_group_size,
            header.chrom_count,
            MetaTotals {
                rows: header.chrom_meta_count,
                numeric: header.chrom_meta_numeric_count,
                string: header.chrom_meta_string_count,
                uncompressed: header.chrom_meta_uncompressed_bytes,
            },
            header.compression_codec,
            config.verify_checksums,
            config.decompression_budget,
            config.max_cached_bytes,
        )?;

        Ok(Self {
            header,
            source,
            spec_segment_bounds: SegmentBoundsCache::Unloaded,
            chrom_segment_bounds: SegmentBoundsCache::Unloaded,
            spec_summary_buf: Arc::from(spec_summary_buf),
            chrom_summary_buf: Arc::from(chrom_summary_buf),
            spec_entries_buf: Arc::from(spec_entries_buf),
            spec_array_refs: Arc::from(spec_array_refs),
            chrom_entries_buf: Arc::from(chrom_entries_buf),
            chrom_array_refs: Arc::from(chrom_array_refs),
            global_meta_buf: Arc::from(global_meta_buf),
            spec_container,
            chrom_container,
            spec_meta_reader,
            chrom_meta_reader,
            mz_values: Vec::new(),
            int_values: Vec::new(),
            parallel: config.parallel,
            decompression_budget: config.decompression_budget,
        })
    }

    #[inline]
    pub fn format_version(&self) -> u16 {
        self.header.format_version
    }

    #[inline]
    pub fn spectrum_count(&self) -> u64 {
        self.header.spectrum_count
    }

    #[inline]
    pub fn chromatogram_count(&self) -> u64 {
        self.header.chrom_count
    }

    pub fn spec_summary(&self, index: usize) -> Option<SpectrumSummary> {
        let b = slice_summary(
            &self.spec_summary_buf,
            0,
            index,
            SPEC_SUMMARY_SIZE,
            self.header.spectrum_count,
        )?;
        Some(parse_spec_summary(b))
    }

    pub fn spec_summaries(&self) -> IonResult<Vec<SpectrumSummary>> {
        let len = usize::try_from(self.header.len_spec_summary)
            .map_err(|_| IonError::from("spec summary: out of bounds"))?;
        let count = usize::try_from(self.header.spectrum_count)
            .map_err(|_| IonError::from("spec summary: out of bounds"))?;
        if len != count * SPEC_SUMMARY_SIZE {
            return Err(
                format!("spec summary: len={len} != count={count} × {SPEC_SUMMARY_SIZE}").into(),
            );
        }
        Ok(self
            .spec_summary_buf
            .chunks_exact(SPEC_SUMMARY_SIZE)
            .map(parse_spec_summary)
            .collect())
    }

    pub fn chrom_summary(&self, index: usize) -> Option<ChromatogramSummary> {
        let b = slice_summary(
            &self.chrom_summary_buf,
            0,
            index,
            CHROM_SUMMARY_SIZE,
            self.header.chrom_count,
        )?;
        Some(parse_chrom_summary(b))
    }

    pub fn chrom_summaries(&self) -> IonResult<Vec<ChromatogramSummary>> {
        let len = usize::try_from(self.header.len_chrom_summary)
            .map_err(|_| IonError::from("chrom summary: out of bounds"))?;
        let count = usize::try_from(self.header.chrom_count)
            .map_err(|_| IonError::from("chrom summary: out of bounds"))?;
        if len != count * CHROM_SUMMARY_SIZE {
            return Err(format!(
                "chrom summary: len={len} != count={count} × {CHROM_SUMMARY_SIZE}"
            )
            .into());
        }
        Ok(self
            .chrom_summary_buf
            .chunks_exact(CHROM_SUMMARY_SIZE)
            .map(parse_chrom_summary)
            .collect())
    }

    pub fn spectrum_array_refs(&self, index: usize) -> Option<Vec<ArrayRef>> {
        if index >= self.header.spectrum_count as usize {
            return None;
        }
        read_array_refs_from_buffers(&self.spec_entries_buf, &self.spec_array_refs, index)
            .map(ArrayRefList::into_vec)
    }

    pub fn chromatogram_array_refs(&self, index: usize) -> Option<Vec<ArrayRef>> {
        if index >= self.header.chrom_count as usize {
            return None;
        }
        read_array_refs_from_buffers(&self.chrom_entries_buf, &self.chrom_array_refs, index)
            .map(ArrayRefList::into_vec)
    }

    pub(crate) fn spec_block_range(&self, block_id: u32) -> Option<(u64, u64)> {
        self.spec_container.block_byte_range(block_id)
    }

    pub(crate) fn chrom_block_range(&self, block_id: u32) -> Option<(u64, u64)> {
        self.chrom_container.as_ref()?.block_byte_range(block_id)
    }

    pub(crate) fn spec_block_ranges_all(&self) -> Vec<(u64, u64)> {
        self.spec_container.all_block_ranges()
    }

    pub(crate) fn mzml_block_ranges(&self) -> Vec<(u64, u64)> {
        let mut ranges = self.spec_container.all_block_ranges();
        if let Some(chrom) = self.chrom_container.as_ref() {
            ranges.extend(chrom.all_block_ranges());
        }
        ranges
    }

    pub(crate) fn spectrum_block_ranges(&self, index: usize) -> Vec<(u64, u64)> {
        let mut ranges = Vec::new();
        if let Some(array_refs) =
            read_array_refs_from_buffers(&self.spec_entries_buf, &self.spec_array_refs, index)
        {
            for array_ref in array_refs.as_slice() {
                if let Some(range) = self.spec_container.block_byte_range(array_ref.block_id) {
                    ranges.push(range);
                }
            }
        }
        ranges
    }

    pub fn read_spectrum_array(
        &mut self,
        array_ref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        let (element_offset, count, stride) = aref_read_params(array_ref);
        let raw = self.spec_container.get_array_bytes_from_block(
            array_ref.block_id,
            element_offset,
            count,
            stride,
            "read_spectrum_array",
        )?;
        decode_into(out, raw, array_ref.dtype, array_ref.array_filter)
    }

    pub fn read_chromatogram_array(
        &mut self,
        array_ref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        let container = self
            .chrom_container
            .as_mut()
            .ok_or_else(|| IonError::from("no chromatogram container"))?;
        let (element_offset, count, stride) = aref_read_params(array_ref);
        let raw = container.get_array_bytes_from_block(
            array_ref.block_id,
            element_offset,
            count,
            stride,
            "read_chromatogram_array",
        )?;
        decode_into(out, raw, array_ref.dtype, array_ref.array_filter)
    }

    fn read_group_values(&mut self, group: &ArrayGroup, out: &mut Vec<f64>) -> IonResult<()> {
        let Some((first, rest)) = group.refs.split_first() else {
            out.clear();
            return Ok(());
        };
        self.read_spectrum_array(first, out)?;
        let mut segment = Vec::new();
        for array_ref in rest {
            self.read_spectrum_array(array_ref, &mut segment)?;
            out.extend_from_slice(&segment);
        }
        Ok(())
    }

    pub(crate) fn global_metadata(&self) -> IonResult<Vec<Metadatum>> {
        parse_global_metadata(
            &self.global_meta_buf,
            0,
            self.header.global_meta_count,
            self.header.global_meta_numeric_count,
            self.header.global_meta_string_count,
            self.header.compression_codec,
            self.header.global_meta_uncompressed_bytes,
            self.decompression_budget,
        )
    }

    pub(crate) fn spectrum_metadata(&self) -> IonResult<Vec<Metadatum>> {
        self.spec_meta_reader.read_all()
    }

    pub(crate) fn chromatogram_metadata(&self) -> IonResult<Vec<Metadatum>> {
        self.chrom_meta_reader.read_all()
    }

    pub fn spectrum_metadata_at(&mut self, index: usize) -> IonResult<Vec<Metadatum>> {
        self.spec_meta_reader.read_item(index as u64)
    }

    pub fn chromatogram_metadata_at(&mut self, index: usize) -> IonResult<Vec<Metadatum>> {
        self.chrom_meta_reader.read_item(index as u64)
    }

    pub fn to_mzml_metadata_only(&self) -> IonResult<MzML> {
        MzmlConverter::metadata_only(self)
    }

    pub fn to_mzml(&mut self) -> IonResult<MzML> {
        MzmlConverter::new(self).full()
    }

    pub fn spectrum_at(&mut self, index: usize) -> IonResult<Option<Spectrum>> {
        if index >= self.header.spectrum_count as usize {
            return Ok(None);
        }
        let rows = self.spec_meta_reader.read_item(index as u64)?;
        let Some(mut spectrum) = build_one_spectrum(&rows, index) else {
            return Ok(None);
        };

        if let Some(array_refs) =
            read_array_refs_from_buffers(&self.spec_entries_buf, &self.spec_array_refs, index)
        {
            let groups = group_arrays(array_refs.as_slice())?;
            let bd_list = spectrum
                .binary_data_array_list
                .get_or_insert_with(BinaryDataArrayList::default);
            for group in groups {
                let decoded = read_group_decoded_bytes(&group, &mut self.spec_container)?;
                attach_logical_array(bd_list, group.array_type, group.dtype, &decoded)?;
            }
            bd_list.count = Some(bd_list.binary_data_arrays.len());
        }
        Ok(Some(spectrum))
    }

    pub fn read_spectrum_logical_array(
        &mut self,
        spectrum_index: usize,
        array_type: u32,
    ) -> IonResult<Vec<f64>> {
        if spectrum_index >= self.header.spectrum_count as usize {
            return Err("spectrum index out of range".into());
        }

        let Some(array_refs) = read_array_refs_from_buffers(
            &self.spec_entries_buf,
            &self.spec_array_refs,
            spectrum_index,
        ) else {
            return Ok(Vec::new());
        };

        let groups = group_arrays(array_refs.as_slice())?;

        for group in groups {
            if group.array_type != array_type {
                continue;
            }

            let mut values = Vec::new();
            self.read_group_values(&group, &mut values)?;
            return Ok(values);
        }

        Ok(Vec::new())
    }

    pub fn read_spectrum_window(
        &mut self,
        index: usize,
        x_array_accession: u32,
        y_array_accession: u32,
        low: f64,
        high: f64,
    ) -> IonResult<ArrayWindow> {
        self.read_spectrum_window_inner(index, x_array_accession, y_array_accession, low, high)
    }

    pub fn read_spectrum_mz_window(
        &mut self,
        index: usize,
        low: f64,
        high: f64,
    ) -> IonResult<SpectrumWindow> {
        let window =
            self.read_spectrum_window(index, crate::accessions::MZ_ARRAY, ACC_INT, low, high)?;

        Ok(SpectrumWindow {
            mz: window.x,
            intensity: window.y,
        })
    }

    fn read_spectrum_window_inner(
        &mut self,
        index: usize,
        x_array_accession: u32,
        y_array_accession: u32,
        low: f64,
        high: f64,
    ) -> IonResult<ArrayWindow> {
        if index >= self.header.spectrum_count as usize {
            return Err("spectrum index out of range".into());
        }

        if low > high {
            return Ok(ArrayWindow::empty());
        }

        let Some(array_refs) =
            read_array_refs_from_buffers(&self.spec_entries_buf, &self.spec_array_refs, index)
        else {
            return Ok(ArrayWindow::empty());
        };
        let Some(ref_start) = array_ref_start_for_item(&self.spec_entries_buf, index) else {
            return Ok(ArrayWindow::empty());
        };

        let groups = group_arrays(array_refs.as_slice())?;

        let mut x_group = None;
        let mut y_group = None;
        let mut position = 0u64;
        for group in &groups {
            if group.array_type == x_array_accession && x_group.is_none() {
                x_group = Some((position, group));
            } else if group.array_type == y_array_accession && y_group.is_none() {
                y_group = Some((position, group));
            }
            position += group.refs.len() as u64;
        }

        let (Some((x_position, x_group)), Some((_, y_group))) = (x_group, y_group) else {
            return Ok(ArrayWindow::empty());
        };

        if x_group.refs.len() == y_group.refs.len() {
            self.ensure_spec_segment_bounds();
            if let Some(window) =
                self.try_fast_window(ref_start + x_position, x_group, y_group, low, high)?
            {
                return Ok(window);
            }
        }

        self.read_full_window(x_group, y_group, low, high)
    }

    fn try_fast_window(
        &mut self,
        x_ref_base: u64,
        x_group: &ArrayGroup,
        y_group: &ArrayGroup,
        low: f64,
        high: f64,
    ) -> IonResult<Option<ArrayWindow>> {
        let kept_segments = {
            let SegmentBoundsCache::Loaded(bounds) = &self.spec_segment_bounds else {
                return Ok(None);
            };
            let mut kept = Vec::with_capacity(x_group.refs.len());
            for segment_index in 0..x_group.refs.len() {
                let global_ref_index = x_ref_base + segment_index as u64;
                let Some((segment_low, segment_high)) = bounds.get(global_ref_index) else {
                    return Ok(None);
                };
                let overlaps_window = segment_low <= high && segment_high >= low;
                if overlaps_window {
                    kept.push(segment_index);
                }
            }
            kept
        };

        let mut x_out = Vec::new();
        let mut y_out = Vec::new();
        let mut x_segment = Vec::new();
        let mut y_segment = Vec::new();
        for segment_index in kept_segments {
            self.read_spectrum_array(&x_group.refs[segment_index], &mut x_segment)?;
            self.read_spectrum_array(&y_group.refs[segment_index], &mut y_segment)?;
            keep_pairs_in_range_sorted(&x_segment, &y_segment, low, high, &mut x_out, &mut y_out);
        }

        Ok(Some(ArrayWindow { x: x_out, y: y_out }))
    }

    fn read_full_window(
        &mut self,
        x_group: &ArrayGroup,
        y_group: &ArrayGroup,
        low: f64,
        high: f64,
    ) -> IonResult<ArrayWindow> {
        let mut x = Vec::new();
        let mut y = Vec::new();
        self.read_group_values(x_group, &mut x)?;
        self.read_group_values(y_group, &mut y)?;

        let mut x_out = Vec::new();
        let mut y_out = Vec::new();
        keep_pairs_in_range(&x, &y, low, high, &mut x_out, &mut y_out);
        Ok(ArrayWindow { x: x_out, y: y_out })
    }

    fn ensure_spec_segment_bounds(&mut self) {
        if !matches!(self.spec_segment_bounds, SegmentBoundsCache::Unloaded) {
            return;
        }
        self.spec_segment_bounds = match self.load_spec_segment_bounds() {
            Some(index) => SegmentBoundsCache::Loaded(index),
            None => SegmentBoundsCache::Absent,
        };
    }

    fn ensure_chrom_segment_bounds(&mut self) {
        if !matches!(self.chrom_segment_bounds, SegmentBoundsCache::Unloaded) {
            return;
        }
        self.chrom_segment_bounds = match self.load_chrom_segment_bounds() {
            Some(index) => SegmentBoundsCache::Loaded(index),
            None => SegmentBoundsCache::Absent,
        };
    }

    fn load_spec_segment_bounds(&self) -> Option<SegmentBoundsIndex> {
        if self.header.len_spec_segment_bounds == 0 {
            return None;
        }
        let spec_array_ref_count = self.spec_array_refs.len() as u64 / ARRAY_REF_BYTES as u64;
        let bytes = self
            .source
            .read(
                self.header.off_spec_segment_bounds,
                self.header.len_spec_segment_bounds,
            )
            .ok()?;
        if crc32fast::hash(&bytes) != self.header.spec_segment_bounds_crc32 {
            return None;
        }
        let decompressed = self
            .decompress_segment_bounds(&bytes, self.header.plain_len_spec_segment_bounds as usize)
            .ok()?;
        SegmentBoundsIndex::from_bytes(&decompressed, spec_array_ref_count).ok()
    }

    fn load_chrom_segment_bounds(&self) -> Option<SegmentBoundsIndex> {
        if self.header.len_chrom_segment_bounds == 0 {
            return None;
        }
        let chrom_array_ref_count = self.chrom_array_refs.len() as u64 / ARRAY_REF_BYTES as u64;
        let bytes = self
            .source
            .read(
                self.header.off_chrom_segment_bounds,
                self.header.len_chrom_segment_bounds,
            )
            .ok()?;
        if crc32fast::hash(&bytes) != self.header.chrom_segment_bounds_crc32 {
            return None;
        }
        let decompressed = self
            .decompress_segment_bounds(&bytes, self.header.plain_len_chrom_segment_bounds as usize)
            .ok()?;
        SegmentBoundsIndex::from_bytes(&decompressed, chrom_array_ref_count).ok()
    }

    fn decompress_segment_bounds(&self, bytes: &[u8], plain_len: usize) -> IonResult<Vec<u8>> {
        match self.header.compression_codec {
            CODEC_NONE => {
                if bytes.len() != plain_len {
                    return Err("segment bounds: uncompressed length mismatch".into());
                }
                Ok(bytes.to_vec())
            }
            CODEC_ZSTD => decompress_zstd(bytes, plain_len, self.decompression_budget),
            _ => Err("segment bounds: unsupported codec".into()),
        }
    }

    pub fn candidate_items_for_axis(
        &mut self,
        target: Target,
        axis_accession: u32,
        lo: f64,
        hi: f64,
    ) -> IonResult<Vec<ItemSlice>> {
        use crate::ion::axes::axis_of;

        if axis_of(axis_accession).is_none() {
            return Ok(Vec::new());
        }

        let (entries_buf, array_refs_buf, segment_bounds) = match target {
            Target::Spec => {
                self.ensure_spec_segment_bounds();
                (
                    &self.spec_entries_buf,
                    &self.spec_array_refs,
                    &self.spec_segment_bounds,
                )
            }
            Target::Chrom => {
                self.ensure_chrom_segment_bounds();
                (
                    &self.chrom_entries_buf,
                    &self.chrom_array_refs,
                    &self.chrom_segment_bounds,
                )
            }
        };

        let item_count = match target {
            Target::Spec => self.header.spectrum_count,
            Target::Chrom => self.header.chrom_count,
        };

        let bounds = match segment_bounds {
            SegmentBoundsCache::Loaded(b) => Some(b),
            _ => None,
        };

        let mut result = Vec::new();

        for item_idx in 0..item_count {
            let entry_offset = (item_idx as usize) * INDEX_ENTRY_BYTES;
            if entry_offset + INDEX_ENTRY_BYTES > entries_buf.len() {
                break;
            }
            let entry = &entries_buf[entry_offset..entry_offset + INDEX_ENTRY_BYTES];
            let first_aref = u64::from_le_bytes(entry[0..8].try_into().unwrap());
            let aref_count = u64::from_le_bytes(entry[8..16].try_into().unwrap());

            for segment_index in 0..aref_count {
                let array_ref_index = first_aref + segment_index;

                let aref_offset = (array_ref_index as usize) * ARRAY_REF_BYTES;
                if aref_offset + ARRAY_REF_BYTES > array_refs_buf.len() {
                    continue;
                }
                let aref_bytes = &array_refs_buf[aref_offset..aref_offset + ARRAY_REF_BYTES];
                let array_type = u32::from_le_bytes(aref_bytes[20..24].try_into().unwrap());

                if array_type != axis_accession {
                    continue;
                }

                let include = match bounds.as_ref() {
                    Some(b) => {
                        if let Some((segment_low, segment_high)) = b.get(array_ref_index) {
                            segment_low <= hi && segment_high >= lo
                        } else {
                            true
                        }
                    }
                    None => true,
                };

                if include {
                    result.push(ItemSlice {
                        item_index: item_idx,
                        array_ref_index,
                    });
                }
            }
        }

        Ok(result)
    }
}

fn slice_is_non_decreasing(values: &[f64]) -> bool {
    values.windows(2).all(|pair| pair[0] <= pair[1])
}

fn keep_pairs_in_range_sorted(
    x: &[f64],
    y: &[f64],
    low: f64,
    high: f64,
    x_out: &mut Vec<f64>,
    y_out: &mut Vec<f64>,
) {
    let paired = x.len().min(y.len());
    let x = &x[..paired];
    let y = &y[..paired];
    let start = x.partition_point(|&value| value < low);
    let end = x.partition_point(|&value| value <= high);
    x_out.extend_from_slice(&x[start..end]);
    y_out.extend_from_slice(&y[start..end]);
}

fn keep_pairs_in_range(
    x: &[f64],
    y: &[f64],
    low: f64,
    high: f64,
    x_out: &mut Vec<f64>,
    y_out: &mut Vec<f64>,
) {
    let paired = x.len().min(y.len());
    let x = &x[..paired];
    let y = &y[..paired];

    if slice_is_non_decreasing(x) {
        keep_pairs_in_range_sorted(x, y, low, high, x_out, y_out);
        return;
    }
    for (position, &value) in x.iter().enumerate() {
        if value >= low && value <= high {
            x_out.push(value);
            y_out.push(y[position]);
        }
    }
}

fn array_ref_start_for_item(entries_buf: &[u8], index: usize) -> Option<u64> {
    let entry_offset = index.checked_mul(INDEX_ENTRY_BYTES)?;
    let entry_end = entry_offset.checked_add(INDEX_ENTRY_BYTES)?;
    let entry = entries_buf.get(entry_offset..entry_end)?;
    Some(u64::from_le_bytes(entry[0..8].try_into().unwrap()))
}

fn build_one_spectrum(rows: &[Metadatum], fallback_index: usize) -> Option<Spectrum> {
    let children_lookup = ChildrenLookup::new(rows);
    let spectrum_id = children_lookup.all_ids(TagId::Spectrum).first().copied()?;
    let mut owner_rows = OwnerRows::with_capacity(rows.len());
    for row in rows {
        owner_rows.insert(row.id, row);
    }
    let policy = DefaultMetadataPolicy;
    let mut param_buffer = Vec::new();
    Some(parse_spectrum(
        &owner_rows,
        &children_lookup,
        spectrum_id,
        fallback_index as u32,
        &policy,
        &mut param_buffer,
    ))
}

#[allow(clippy::large_enum_variant)]
enum IonBackend {
    Decoder(Decoder),
    Async(AsyncReader),
    Data,
}

impl IonBackend {
    fn as_decoder_mut(&mut self) -> Option<&mut Decoder> {
        match self {
            Self::Decoder(d) => Some(d),
            Self::Async(_) => None,
            Self::Data => None,
        }
    }
}

pub struct Ion {
    pub cv_list: Option<CvList>,
    pub file_description: Option<FileDescription>,
    pub referenceable_param_group_list: Option<ReferenceableParamGroupList>,
    pub sample_list: Option<SampleList>,
    pub instrument_list: Option<InstrumentList>,
    pub software_list: Option<SoftwareList>,
    pub data_processing_list: Option<DataProcessingList>,
    pub scan_settings_list: Option<ScanSettingsList>,
    pub run: Run,
    backend: IonBackend,
}

impl Ion {
    pub fn open(bytes: &[u8], config: DecoderConfig) -> IonResult<Self> {
        let decoder = Decoder::open(bytes, config)?;
        Ok(Self::empty(IonBackend::Decoder(decoder)))
    }

    pub fn open_arc(data: Arc<[u8]>, config: DecoderConfig) -> IonResult<Self> {
        let decoder = Decoder::open_arc(data, config)?;
        Ok(Self::empty(IonBackend::Decoder(decoder)))
    }

    pub fn open_with_source(source: Arc<dyn ByteSource>, config: DecoderConfig) -> IonResult<Self> {
        let decoder = Decoder::open_with_source(source, config)?;
        Ok(Self::empty(IonBackend::Decoder(decoder)))
    }

    pub fn open_with_query(
        read: impl Fn(Query) -> IonResult<QueryPayload> + Send + Sync + 'static,
        config: DecoderConfig,
    ) -> IonResult<Self> {
        let decoder = Decoder::open_with_query(read, config)?;
        Ok(Self::empty(IonBackend::Decoder(decoder)))
    }

    pub async fn open_with_async_source(
        source: Arc<dyn AsyncByteSource>,
        config: DecoderConfig,
    ) -> IonResult<Self> {
        let reader = AsyncReader::open_with_async_source(source, config).await?;
        Ok(Self::empty(IonBackend::Async(reader)))
    }

    pub async fn open_with_async_query(
        read: impl Fn(Query) -> QueryPromise<'static> + 'static,
        config: DecoderConfig,
    ) -> IonResult<Self> {
        let reader = AsyncReader::open_with_async_query(read, config).await?;
        Ok(Self::empty(IonBackend::Async(reader)))
    }

    pub fn open_bytes(bytes: Arc<[u8]>, config: DecoderConfig) -> IonResult<OwnedIon> {
        OwnedIon::open_bytes(bytes, config)
    }

    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    pub fn open_file(path: &Path, config: DecoderConfig) -> IonResult<OwnedIon> {
        OwnedIon::open(path, config)
    }

    pub fn from_mzml(mzml: MzML) -> Self {
        let mut ion = Self::empty(IonBackend::Data);
        ion.set_from_mzml(mzml);
        ion
    }

    #[inline]
    fn empty(backend: IonBackend) -> Self {
        Self {
            cv_list: None,
            file_description: None,
            referenceable_param_group_list: None,
            sample_list: None,
            instrument_list: None,
            software_list: None,
            data_processing_list: None,
            scan_settings_list: None,
            run: Run::default(),
            backend,
        }
    }

    fn set_from_mzml(&mut self, mzml: MzML) {
        self.cv_list = mzml.cv_list;
        self.file_description = mzml.file_description;
        self.referenceable_param_group_list = mzml.referenceable_param_group_list;
        self.sample_list = mzml.sample_list;
        self.instrument_list = mzml.instrument_list;
        self.software_list = mzml.software_list;
        self.data_processing_list = mzml.data_processing_list;
        self.scan_settings_list = mzml.scan_settings_list;
        self.run = mzml.run;
    }

    #[inline]
    fn clone_as_mzml(&self) -> MzML {
        MzML {
            cv_list: self.cv_list.clone(),
            file_description: self.file_description.clone(),
            referenceable_param_group_list: self.referenceable_param_group_list.clone(),
            sample_list: self.sample_list.clone(),
            instrument_list: self.instrument_list.clone(),
            software_list: self.software_list.clone(),
            data_processing_list: self.data_processing_list.clone(),
            scan_settings_list: self.scan_settings_list.clone(),
            run: self.run.clone(),
        }
    }

    fn clone_as_mzml_metadata_only(&self) -> MzML {
        let mut run = self.run.clone();
        run.spectrum_list = None;
        run.chromatogram_list = None;
        MzML {
            cv_list: self.cv_list.clone(),
            file_description: self.file_description.clone(),
            referenceable_param_group_list: self.referenceable_param_group_list.clone(),
            sample_list: self.sample_list.clone(),
            instrument_list: self.instrument_list.clone(),
            software_list: self.software_list.clone(),
            data_processing_list: self.data_processing_list.clone(),
            scan_settings_list: self.scan_settings_list.clone(),
            run,
        }
    }

    pub fn load_metadata(&mut self) -> IonResult<()> {
        let mzml = match &mut self.backend {
            IonBackend::Decoder(decoder) => Some(decoder.to_mzml_metadata_only()?),
            IonBackend::Async(reader) => Some(reader.decoder().to_mzml_metadata_only()?),
            IonBackend::Data => None,
        };
        if let Some(mzml) = mzml {
            self.set_from_mzml(mzml);
        }
        Ok(())
    }

    #[inline]
    pub fn spectrum_count(&self) -> u64 {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.spectrum_count(),
            IonBackend::Async(reader) => reader.decoder().spectrum_count(),
            IonBackend::Data => self
                .run
                .spectrum_list
                .as_ref()
                .map_or(0, |l| l.spectra.len() as u64),
        }
    }

    #[inline]
    pub fn chromatogram_count(&self) -> u64 {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.chromatogram_count(),
            IonBackend::Async(reader) => reader.decoder().chromatogram_count(),
            IonBackend::Data => self
                .run
                .chromatogram_list
                .as_ref()
                .map_or(0, |l| l.chromatograms.len() as u64),
        }
    }

    #[inline]
    pub fn format_version(&self) -> Option<u16> {
        match &self.backend {
            IonBackend::Decoder(decoder) => Some(decoder.format_version()),
            IonBackend::Async(reader) => Some(reader.decoder().format_version()),
            IonBackend::Data => None,
        }
    }

    #[inline]
    pub fn spec_summary(&self, index: usize) -> Option<SpectrumSummary> {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.spec_summary(index),
            IonBackend::Async(reader) => reader.decoder().spec_summary(index),
            IonBackend::Data => None,
        }
    }

    pub fn spec_summaries(&self) -> IonResult<Vec<SpectrumSummary>> {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.spec_summaries(),
            IonBackend::Async(reader) => reader.decoder().spec_summaries(),
            IonBackend::Data => Err(IonError::from(
                "spec summary summaries are unavailable for mzML-backed Ion",
            )),
        }
    }

    #[inline]
    pub fn chrom_summary(&self, index: usize) -> Option<ChromatogramSummary> {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.chrom_summary(index),
            IonBackend::Async(reader) => reader.decoder().chrom_summary(index),
            IonBackend::Data => None,
        }
    }

    pub fn chrom_summaries(&self) -> IonResult<Vec<ChromatogramSummary>> {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.chrom_summaries(),
            IonBackend::Async(reader) => reader.decoder().chrom_summaries(),
            IonBackend::Data => Err(IonError::from(
                "chrom summary summaries are unavailable for mzML-backed Ion",
            )),
        }
    }

    pub fn spectrum_array_refs(&self, index: usize) -> Option<Vec<ArrayRef>> {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.spectrum_array_refs(index),
            IonBackend::Async(reader) => reader.decoder().spectrum_array_refs(index),
            IonBackend::Data => None,
        }
    }

    pub fn chromatogram_array_refs(&self, index: usize) -> Option<Vec<ArrayRef>> {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.chromatogram_array_refs(index),
            IonBackend::Async(reader) => reader.decoder().chromatogram_array_refs(index),
            IonBackend::Data => None,
        }
    }

    pub fn read_spectrum_array(
        &mut self,
        array_ref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        self.backend
            .as_decoder_mut()
            .ok_or_else(|| {
                IonError::from(
                    "read_spectrum_array needs a sync source; for an async-backed Ion use read_spectrum_array_async",
                )
            })
            .and_then(|d| d.read_spectrum_array(array_ref, out))
    }

    pub fn read_chromatogram_array(
        &mut self,
        array_ref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        self.backend
            .as_decoder_mut()
            .ok_or_else(|| {
                IonError::from(
                    "read_chromatogram_array needs a sync source; for an async-backed Ion use read_chromatogram_array_async",
                )
            })
            .and_then(|d| d.read_chromatogram_array(array_ref, out))
    }

    pub fn read_spectrum_window(
        &mut self,
        index: usize,
        x_array_accession: u32,
        y_array_accession: u32,
        low: f64,
        high: f64,
    ) -> IonResult<ArrayWindow> {
        self.backend
            .as_decoder_mut()
            .ok_or_else(|| {
                IonError::from(
                    "read_spectrum_window needs a sync source; for an async-backed Ion use the decoder directly",
                )
            })
            .and_then(|d| {
                d.read_spectrum_window(index, x_array_accession, y_array_accession, low, high)
            })
    }

    pub fn read_spectrum_mz_window(
        &mut self,
        index: usize,
        low: f64,
        high: f64,
    ) -> IonResult<SpectrumWindow> {
        self.backend
            .as_decoder_mut()
            .ok_or_else(|| {
                IonError::from(
                    "read_spectrum_mz_window needs a sync source; for an async-backed Ion use the decoder directly",
                )
            })
            .and_then(|d| d.read_spectrum_mz_window(index, low, high))
    }

    pub fn read_spectrum_logical_array(
        &mut self,
        index: usize,
        array_type: u32,
    ) -> IonResult<Vec<f64>> {
        self.backend
            .as_decoder_mut()
            .ok_or_else(|| {
                IonError::from(
                    "read_spectrum_logical_array needs a sync source; for an async-backed Ion use the decoder directly",
                )
            })
            .and_then(|d| d.read_spectrum_logical_array(index, array_type))
    }

    pub async fn read_spectrum_array_async(
        &mut self,
        array_ref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => decoder.read_spectrum_array(array_ref, out),
            IonBackend::Async(reader) => reader.read_spectrum_array(array_ref, out).await,
            IonBackend::Data => Err(IonError::from(
                "array refs are unavailable for mzML-backed Ion",
            )),
        }
    }

    pub async fn read_chromatogram_array_async(
        &mut self,
        array_ref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => decoder.read_chromatogram_array(array_ref, out),
            IonBackend::Async(reader) => reader.read_chromatogram_array(array_ref, out).await,
            IonBackend::Data => Err(IonError::from(
                "array refs are unavailable for mzML-backed Ion",
            )),
        }
    }

    pub fn to_mzml(&mut self) -> IonResult<MzML> {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => decoder.to_mzml(),
            IonBackend::Async(_) => {
                Err(IonError::from("use to_mzml_async for an async-backed Ion"))
            }
            IonBackend::Data => Ok(self.clone_as_mzml()),
        }
    }

    pub async fn to_mzml_async(&mut self) -> IonResult<MzML> {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => decoder.to_mzml(),
            IonBackend::Async(reader) => reader.to_mzml().await,
            IonBackend::Data => Ok(self.clone_as_mzml()),
        }
    }

    pub fn to_mzml_metadata_only(&self) -> IonResult<MzML> {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.to_mzml_metadata_only(),
            IonBackend::Async(reader) => reader.decoder().to_mzml_metadata_only(),
            IonBackend::Data => Ok(self.clone_as_mzml_metadata_only()),
        }
    }

    pub fn spectrum_at(&mut self, index: usize) -> IonResult<Option<Spectrum>> {
        match &mut self.backend {
            IonBackend::Decoder(d) => d.spectrum_at(index),
            IonBackend::Async(_) => Err(IonError::from(
                "use spectrum_at_async for an async-backed Ion",
            )),
            IonBackend::Data => Ok(self
                .run
                .spectrum_list
                .as_ref()
                .and_then(|l| l.spectra.get(index).cloned())),
        }
    }

    pub async fn spectrum_at_async(&mut self, index: usize) -> IonResult<Option<Spectrum>> {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => decoder.spectrum_at(index),
            IonBackend::Async(reader) => reader.spectrum_at(index).await,
            IonBackend::Data => Ok(self
                .run
                .spectrum_list
                .as_ref()
                .and_then(|l| l.spectra.get(index).cloned())),
        }
    }

    pub fn spectrum_metadata_at(&mut self, index: usize) -> IonResult<Vec<Metadatum>> {
        match &mut self.backend {
            IonBackend::Decoder(d) => d.spectrum_metadata_at(index),
            IonBackend::Async(reader) => reader.decoder_mut().spectrum_metadata_at(index),
            IonBackend::Data => Err(IonError::from(
                "metadata rows are only available on a file-backed Ion (use Ion::open)",
            )),
        }
    }

    pub fn chromatogram_metadata_at(&mut self, index: usize) -> IonResult<Vec<Metadatum>> {
        match &mut self.backend {
            IonBackend::Decoder(d) => d.chromatogram_metadata_at(index),
            IonBackend::Async(reader) => reader.decoder_mut().chromatogram_metadata_at(index),
            IonBackend::Data => Err(IonError::from(
                "metadata rows are only available on a file-backed Ion (use Ion::open)",
            )),
        }
    }

    pub async fn load_scan_async(
        &mut self,
        index: usize,
        mz: &mut Vec<f64>,
        intensity: &mut Vec<f64>,
    ) -> IonResult<bool> {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => Ok(decoder.load_scan(index, mz, intensity)),
            IonBackend::Async(reader) => reader.load_scan(index, mz, intensity).await,
            IonBackend::Data => {
                let spectra = self
                    .run
                    .spectrum_list
                    .as_ref()
                    .map(|list| list.spectra.as_slice())
                    .unwrap_or_default();
                Ok(load_scan_from_spectra(spectra, index, mz, intensity))
            }
        }
    }

    pub async fn for_each_in_range_async<F>(
        &mut self,
        rt_min: f64,
        rt_max: f64,
        ms_level: u8,
        callback: F,
    ) -> IonResult<()>
    where
        F: FnMut(&ScanSummary, &[f64], &[f64]),
    {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => {
                decoder.for_each_in_range(rt_min, rt_max, ms_level, callback);
                Ok(())
            }
            IonBackend::Async(reader) => {
                reader
                    .for_each_in_range(rt_min, rt_max, ms_level, callback)
                    .await
            }
            IonBackend::Data => {
                let spectra = self
                    .run
                    .spectrum_list
                    .as_ref()
                    .map(|list| list.spectra.as_slice())
                    .unwrap_or_default();
                let mut mz = Vec::new();
                let mut intensity = Vec::new();
                let mut callback = callback;
                for (index, spectrum) in spectra.iter().enumerate() {
                    let summary = summary_from_spectrum(spectrum);
                    if summary.rt < rt_min
                        || summary.rt > rt_max
                        || (ms_level != 0 && summary.ms_level != ms_level)
                    {
                        continue;
                    }
                    if load_scan_from_spectra(spectra, index, &mut mz, &mut intensity) {
                        callback(&summary, &mz, &intensity);
                    }
                }
                Ok(())
            }
        }
    }
}

pub struct OwnedIon(Ion);

impl OwnedIon {
    pub fn open_bytes(data: Arc<[u8]>, config: DecoderConfig) -> IonResult<Self> {
        Ion::open_arc(data, config).map(OwnedIon)
    }

    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    pub fn open(path: &Path, config: DecoderConfig) -> IonResult<Self> {
        let file = std::fs::File::open(path).map_err(|e| IonError::from(e.to_string()))?;
        let map = unsafe { memmap2::Mmap::map(&file).map_err(|e| IonError::from(e.to_string()))? };
        let source = Arc::new(MmapSource::new(map)) as Arc<dyn ByteSource>;
        Ion::open_with_source(source, config).map(OwnedIon)
    }

    pub fn format_version(&self) -> Option<u16> {
        self.0.format_version()
    }
}

impl Deref for OwnedIon {
    type Target = Ion;

    fn deref(&self) -> &Ion {
        &self.0
    }
}

impl DerefMut for OwnedIon {
    fn deref_mut(&mut self) -> &mut Ion {
        &mut self.0
    }
}

impl ScanSource for Decoder {
    fn for_each_summary(&mut self, callback: &mut dyn FnMut(usize, ScanSummary)) {
        for (index, chunk) in self
            .spec_summary_buf
            .chunks_exact(SPEC_SUMMARY_SIZE)
            .enumerate()
        {
            let summary = parse_spec_summary(chunk);
            callback(
                index,
                ScanSummary {
                    rt: summary.rt_seconds / 60.0,
                    base_peak_mz: summary.base_peak_mz,
                    selected_ion_mz: summary.selected_ion_mz,
                    base_peak_int: summary.base_peak_int,
                    total_ion_current: summary.total_ion_current,
                    ms_level: summary.ms_level,
                    polarity: summary.polarity,
                },
            );
        }
    }

    fn load_scan(&mut self, index: usize, mz: &mut Vec<f64>, intensity: &mut Vec<f64>) -> bool {
        let count = match usize::try_from(self.header.spectrum_count) {
            Ok(c) => c,
            Err(_) => return false,
        };
        if index >= count {
            return false;
        }
        let all_entries = self.spec_entries_buf.as_ref();
        let array_ref_bytes = self.spec_array_refs.as_ref();
        let entry_start = index * INDEX_ENTRY_BYTES;
        let Some(entry) = all_entries.get(entry_start..entry_start + INDEX_ENTRY_BYTES) else {
            return false;
        };
        let Some((mz_ref, int_ref)) = parse_array_pair(entry, array_ref_bytes) else {
            return false;
        };
        if !decode_from_block(&mut self.spec_container, mz, &mz_ref) {
            return false;
        }
        if !decode_from_block(&mut self.spec_container, intensity, &int_ref) {
            return false;
        }
        mz.len().min(intensity.len()) > 0
    }

    fn for_each_in_range<F>(&mut self, rt_min: f64, rt_max: f64, ms_level: u8, mut callback: F)
    where
        Self: Sized,
        F: FnMut(&ScanSummary, &[f64], &[f64]),
    {
        let summary_bytes = self.spec_summary_buf.as_ref();
        let _count = match usize::try_from(self.header.spectrum_count) {
            Ok(count) => count,
            Err(_) => return,
        };
        let entry_bytes = self.spec_entries_buf.as_ref();
        let array_ref_bytes = self.spec_array_refs.as_ref();
        let (container, mz_values, int_values) = (
            &mut self.spec_container as &mut dyn ContainerAccess,
            &mut self.mz_values,
            &mut self.int_values,
        );
        ScanIterator::new(
            summary_bytes,
            entry_bytes,
            array_ref_bytes,
            container,
            mz_values,
            int_values,
            rt_min * 60.0,
            rt_max * 60.0,
            ms_level,
        )
        .run(&mut callback);
    }
}

impl ScanSource for Ion {
    fn for_each_summary(&mut self, callback: &mut dyn FnMut(usize, ScanSummary)) {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => decoder.for_each_summary(callback),
            IonBackend::Async(reader) => {
                let count = match usize::try_from(reader.decoder().spectrum_count()) {
                    Ok(count) => count,
                    Err(_) => return,
                };
                for index in 0..count {
                    if let Some(summary) = reader.decoder().spec_summary(index) {
                        callback(
                            index,
                            ScanSummary {
                                rt: summary.rt_seconds / 60.0,
                                base_peak_mz: summary.base_peak_mz,
                                selected_ion_mz: summary.selected_ion_mz,
                                base_peak_int: summary.base_peak_int,
                                total_ion_current: summary.total_ion_current,
                                ms_level: summary.ms_level,
                                polarity: summary.polarity,
                            },
                        );
                    }
                }
            }
            IonBackend::Data => {
                if let Some(list) = self.run.spectrum_list.as_ref() {
                    summary_from_spectra(&list.spectra, callback);
                }
            }
        }
    }

    fn load_scan(&mut self, index: usize, mz: &mut Vec<f64>, intensity: &mut Vec<f64>) -> bool {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => decoder.load_scan(index, mz, intensity),
            IonBackend::Async(_) => false,
            IonBackend::Data => {
                let spectra = self
                    .run
                    .spectrum_list
                    .as_ref()
                    .map(|l| l.spectra.as_slice())
                    .unwrap_or_default();
                load_scan_from_spectra(spectra, index, mz, intensity)
            }
        }
    }

    fn for_each_in_range<F>(&mut self, rt_min: f64, rt_max: f64, ms_level: u8, callback: F)
    where
        Self: Sized,
        F: FnMut(&ScanSummary, &[f64], &[f64]),
    {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => {
                decoder.for_each_in_range(rt_min, rt_max, ms_level, callback);
            }
            IonBackend::Async(_) => {}
            IonBackend::Data => {
                let spectra = self
                    .run
                    .spectrum_list
                    .as_ref()
                    .map(|l| l.spectra.as_slice())
                    .unwrap_or_default();
                let mut mz = Vec::new();
                let mut intensity = Vec::new();
                let mut cb = callback;
                for (index, spectrum) in spectra.iter().enumerate() {
                    let summary = summary_from_spectrum(spectrum);
                    if summary.rt < rt_min
                        || summary.rt > rt_max
                        || (ms_level != 0 && summary.ms_level != ms_level)
                    {
                        continue;
                    }
                    if load_scan_from_spectra(spectra, index, &mut mz, &mut intensity) {
                        cb(&summary, &mz, &intensity);
                    }
                }
            }
        }
    }
}

struct MzmlConverter<'d> {
    decoder: &'d mut Decoder,
}

impl<'d> MzmlConverter<'d> {
    #[inline]
    fn new(decoder: &'d mut Decoder) -> Self {
        Self { decoder }
    }

    fn metadata_only(decoder: &Decoder) -> IonResult<MzML> {
        let global_meta = decoder.global_metadata()?;
        let global_lookup = ChildrenLookup::new(&global_meta);
        let meta_refs: Vec<&Metadatum> = global_meta.iter().collect();
        let policy = DefaultMetadataPolicy;

        let mut owner_rows = OwnerRows::with_capacity(global_meta.len());
        for metadatum in &global_meta {
            owner_rows.insert(metadatum.id, metadatum);
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

        let spec_meta = decoder.spectrum_metadata()?;
        let chrom_meta = decoder.chromatogram_metadata()?;
        let spec_refs: Vec<&Metadatum> = spec_meta.iter().collect();
        let chrom_refs: Vec<&Metadatum> = chrom_meta.iter().collect();

        let spectrum_list =
            parse_spectrum_list(&spec_refs, &ChildrenLookup::new(&spec_meta), &policy);
        let chromatogram_list =
            parse_chromatogram_list(&chrom_refs, &ChildrenLookup::new(&chrom_meta), &policy);

        let source_file_ref_list = parse_run_source_file_refs(&owner_rows, &global_lookup, run_id);

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
            run: Run {
                id: get_attr_text(rows, ACC_ATTR_ID).unwrap_or_default(),
                start_time_stamp: get_attr_text(rows, ACC_ATTR_START_TIME_STAMP)
                    .filter(|value| !value.is_empty()),
                default_instrument_configuration_ref: get_attr_text(
                    rows,
                    ACC_ATTR_DEFAULT_INSTRUMENT_CONFIGURATION_REF,
                )
                .or_else(|| get_attr_text(rows, ACC_ATTR_INSTRUMENT_CONFIGURATION_REF)),
                default_source_file_ref: get_attr_text(rows, ACC_ATTR_DEFAULT_SOURCE_FILE_REF),
                sample_ref: get_attr_text(rows, ACC_ATTR_SAMPLE_REF),
                referenceable_param_group_refs: global_lookup
                    .ids_for(run_id, TagId::ReferenceableParamGroupRef)
                    .iter()
                    .filter_map(|&ref_id| {
                        get_attr_text(owner_rows.get(ref_id), ACC_ATTR_REF)
                            .map(|r| ReferenceableParamGroupRef { r#ref: r })
                    })
                    .collect(),
                cv_params,
                user_params,
                source_file_ref_list,
                spectrum_list,
                chromatogram_list,
            },
        })
    }

    fn full(&mut self) -> IonResult<MzML> {
        let mut mzml = Self::metadata_only(self.decoder)?;

        if let Some(spectrum_list) = mzml.run.spectrum_list.as_mut() {
            attach_binaries(
                self.decoder.spec_entries_buf.as_ref(),
                self.decoder.spec_array_refs.as_ref(),
                &mut spectrum_list.spectra,
                &self.decoder.spec_container,
                "spec",
                self.decoder.parallel,
            )?;
        }

        if let (Some(chrom_list), Some(container)) = (
            mzml.run.chromatogram_list.as_mut(),
            self.decoder.chrom_container.as_ref(),
        ) {
            attach_binaries(
                self.decoder.chrom_entries_buf.as_ref(),
                self.decoder.chrom_array_refs.as_ref(),
                &mut chrom_list.chromatograms,
                container,
                "chrom",
                self.decoder.parallel,
            )?;
        }

        Ok(mzml)
    }
}

struct ScanIterator<'a, 'd> {
    summary_chunks: std::slice::ChunksExact<'a, u8>,
    entry_chunks: std::slice::ChunksExact<'a, u8>,
    aref_bytes: &'a [u8],
    container: &'d mut dyn ContainerAccess,
    mz_values: &'d mut Vec<f64>,
    int_values: &'d mut Vec<f64>,
    rt_min: f64,
    rt_max: f64,
    ms_level: u8,
}

impl<'a, 'd> ScanIterator<'a, 'd> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        summary_bytes: &'a [u8],
        entry_bytes: &'a [u8],
        aref_bytes: &'a [u8],
        container: &'d mut dyn ContainerAccess,
        mz_values: &'d mut Vec<f64>,
        int_values: &'d mut Vec<f64>,
        rt_min: f64,
        rt_max: f64,
        ms_level: u8,
    ) -> Self {
        Self {
            summary_chunks: summary_bytes.chunks_exact(SPEC_SUMMARY_SIZE),
            entry_chunks: entry_bytes.chunks_exact(INDEX_ENTRY_BYTES),
            aref_bytes,
            container,
            mz_values,
            int_values,
            rt_min,
            rt_max,
            ms_level,
        }
    }

    fn run<F>(&mut self, callback: &mut F)
    where
        F: FnMut(&ScanSummary, &[f64], &[f64]),
    {
        for (summary_bytes, entry_bytes) in
            self.summary_chunks.by_ref().zip(self.entry_chunks.by_ref())
        {
            let summary = parse_spec_summary(summary_bytes);
            if !summary.rt_seconds.is_finite()
                || summary.rt_seconds < self.rt_min
                || summary.rt_seconds > self.rt_max
            {
                continue;
            }
            if self.ms_level != 0 && summary.ms_level != self.ms_level {
                continue;
            }
            let Some((mz_ref, int_ref)) = parse_array_pair(entry_bytes, self.aref_bytes) else {
                continue;
            };
            if !decode_from_block(self.container, self.mz_values, &mz_ref) {
                continue;
            }
            if !decode_from_block(self.container, self.int_values, &int_ref) {
                continue;
            }
            let len = self.mz_values.len().min(self.int_values.len());
            if len == 0 {
                continue;
            }
            let summary = ScanSummary {
                rt: summary.rt_seconds / 60.0,
                ms_level: summary.ms_level,
                polarity: summary.polarity,
                base_peak_mz: summary.base_peak_mz,
                selected_ion_mz: summary.selected_ion_mz,
                base_peak_int: summary.base_peak_int,
                total_ion_current: summary.total_ion_current,
            };
            callback(&summary, &self.mz_values[..len], &self.int_values[..len]);
        }
    }
}

trait BinaryArrayOwner {
    fn binary_data_array_list_mut(&mut self) -> &mut Option<BinaryDataArrayList>;
}

impl BinaryArrayOwner for Spectrum {
    #[inline]
    fn binary_data_array_list_mut(&mut self) -> &mut Option<BinaryDataArrayList> {
        &mut self.binary_data_array_list
    }
}

impl BinaryArrayOwner for Chromatogram {
    #[inline]
    fn binary_data_array_list_mut(&mut self) -> &mut Option<BinaryDataArrayList> {
        &mut self.binary_data_array_list
    }
}

#[derive(Clone)]
struct ArrayRefList {
    len: usize,
    inline: [ArrayRef; INLINE_ARRAY_REF_CAP],
    heap: Option<Vec<ArrayRef>>,
}

impl ArrayRefList {
    #[inline]
    fn with_capacity(capacity: usize) -> Self {
        Self {
            len: 0,
            inline: [ArrayRef::default(); INLINE_ARRAY_REF_CAP],
            heap: (capacity > INLINE_ARRAY_REF_CAP).then(|| Vec::with_capacity(capacity)),
        }
    }

    #[inline]
    fn push(&mut self, value: ArrayRef) {
        if let Some(heap) = self.heap.as_mut() {
            heap.push(value);
            self.len = heap.len();
            return;
        }
        self.inline[self.len] = value;
        self.len += 1;
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    fn as_slice(&self) -> &[ArrayRef] {
        match self.heap.as_deref() {
            Some(heap) => heap,
            None => &self.inline[..self.len],
        }
    }

    #[inline]
    fn into_vec(self) -> Vec<ArrayRef> {
        self.heap
            .unwrap_or_else(|| self.inline[..self.len].to_vec())
    }
}

#[inline]
fn slice_summary(bytes: &[u8], off: u64, index: usize, size: usize, count: u64) -> Option<&[u8]> {
    if index >= count as usize {
        return None;
    }
    let base = usize::try_from(off)
        .ok()
        .and_then(|o| index.checked_mul(size).and_then(|d| o.checked_add(d)))?;
    bytes.get(base..base.checked_add(size)?)
}

#[inline]
fn parse_spec_summary(bytes: &[u8]) -> SpectrumSummary {
    SpectrumSummary {
        rt_seconds: f64::from_le_bytes(bytes[0..8].try_into().unwrap()),
        base_peak_mz: f64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        selected_ion_mz: f64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        base_peak_int: f64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        total_ion_current: f64::from_le_bytes(bytes[32..40].try_into().unwrap()),
        ms_level: bytes[40],
        polarity: bytes[41],
        position_x: u32::from_le_bytes(bytes[42..46].try_into().unwrap()),
        position_y: u32::from_le_bytes(bytes[46..50].try_into().unwrap()),
        position_z: u32::from_le_bytes(bytes[50..54].try_into().unwrap()),
    }
}

fn parse_chrom_summary(bytes: &[u8]) -> ChromatogramSummary {
    ChromatogramSummary {
        lowest_mz: f64::from_le_bytes(bytes[0..8].try_into().unwrap()),
        highest_mz: f64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        lowest_wavelength: f64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        highest_wavelength: f64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        lowest_ion_mobility: f64::from_le_bytes(bytes[32..40].try_into().unwrap()),
        highest_ion_mobility: f64::from_le_bytes(bytes[40..48].try_into().unwrap()),
        polarity: bytes[48],
    }
}

#[inline]
fn read_array_refs_from_buffers(
    entries_buf: &[u8],
    arrayrefs_buf: &[u8],
    index: usize,
) -> Option<ArrayRefList> {
    let entry_offset = index.checked_mul(INDEX_ENTRY_BYTES)?;
    let entry_end = entry_offset.checked_add(INDEX_ENTRY_BYTES)?;
    let entry = entries_buf.get(entry_offset..entry_end)?;
    let ref_start = usize::try_from(u64::from_le_bytes(entry[0..8].try_into().unwrap())).ok()?;
    let ref_count = usize::try_from(u64::from_le_bytes(entry[8..16].try_into().unwrap())).ok()?;
    let max_refs = arrayrefs_buf.len() / ARRAY_REF_BYTES;
    if ref_count > max_refs {
        return None;
    }
    let mut refs = ArrayRefList::with_capacity(ref_count);
    for offset in 0..ref_count {
        let pos = ref_start
            .checked_add(offset)?
            .checked_mul(ARRAY_REF_BYTES)?;
        let end = pos.checked_add(ARRAY_REF_BYTES)?;
        refs.push(parse_array_ref(arrayrefs_buf.get(pos..end)?));
    }
    Some(refs)
}

fn read_array_refs_at(
    bytes: &[u8],
    entry_base: usize,
    aref_base: usize,
    index: usize,
) -> Option<ArrayRefList> {
    let entry_offset = index
        .checked_mul(INDEX_ENTRY_BYTES)?
        .checked_add(entry_base)?;
    let entry_end = entry_offset.checked_add(INDEX_ENTRY_BYTES)?;
    let entry = bytes.get(entry_offset..entry_end)?;
    let ref_start = usize::try_from(u64::from_le_bytes(entry[0..8].try_into().unwrap())).ok()?;
    let ref_count = usize::try_from(u64::from_le_bytes(entry[8..16].try_into().unwrap())).ok()?;
    let max_refs = bytes.len().saturating_sub(aref_base) / ARRAY_REF_BYTES;
    if ref_count > max_refs {
        return None;
    }
    let mut refs = ArrayRefList::with_capacity(ref_count);
    for offset in 0..ref_count {
        let pos = ref_start
            .checked_add(offset)?
            .checked_mul(ARRAY_REF_BYTES)?
            .checked_add(aref_base)?;
        let end = pos.checked_add(ARRAY_REF_BYTES)?;
        refs.push(parse_array_ref(bytes.get(pos..end)?));
    }
    Some(refs)
}

#[inline]
fn parse_array_ref(bytes: &[u8]) -> ArrayRef {
    ArrayRef {
        element_offset: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
        element_count: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        block_id: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
        array_type: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
        dtype: bytes[24],
        array_filter: bytes[25],
        encoded_len: u32::from_le_bytes(bytes[26..30].try_into().unwrap()),
        continues_previous_segment: bytes[30],
    }
}

pub(crate) fn group_arrays(refs: &[ArrayRef]) -> IonResult<Vec<ArrayGroup>> {
    if refs.is_empty() {
        return Ok(Vec::new());
    }

    if refs[0].continues_previous_segment != 0 {
        return Err("array grouping: first ref must have continues_previous_segment = 0".into());
    }

    let mut groups = Vec::new();
    let mut current_group_refs = Vec::new();
    let mut current_type = refs[0].array_type;
    let mut current_dtype = refs[0].dtype;
    let mut current_filter = refs[0].array_filter;

    for aref in refs {
        if aref.continues_previous_segment != 0 && aref.continues_previous_segment != 1 {
            return Err(format!(
                "array grouping: invalid continues_previous_segment value {}, must be 0 or 1",
                aref.continues_previous_segment
            )
            .into());
        }

        if aref.continues_previous_segment == 0 {
            if !current_group_refs.is_empty() {
                groups.push(ArrayGroup {
                    array_type: current_type,
                    dtype: current_dtype,
                    array_filter: current_filter,
                    refs: current_group_refs,
                });
                current_group_refs = Vec::new();
            }
            current_type = aref.array_type;
            current_dtype = aref.dtype;
            current_filter = aref.array_filter;
        } else if aref.array_type != current_type
            || aref.dtype != current_dtype
            || aref.array_filter != current_filter
        {
            return Err(
                "array grouping: continuation ref has different array_type, dtype, or filter"
                    .into(),
            );
        }

        current_group_refs.push(*aref);
    }

    if !current_group_refs.is_empty() {
        groups.push(ArrayGroup {
            array_type: current_type,
            dtype: current_dtype,
            array_filter: current_filter,
            refs: current_group_refs,
        });
    }

    for group in &groups {
        if group.refs.len() > 1 {
            for aref in &group.refs {
                if aref.encoded_len > 0 {
                    return Err(
                        "array grouping: multi-ref group cannot contain variable-length arrays"
                            .into(),
                    );
                }
            }
        }
    }

    Ok(groups)
}

pub(crate) fn read_group_decoded_bytes(
    group: &ArrayGroup,
    container: &mut ContainerView<DefaultBlockProcessor>,
) -> IonResult<Vec<u8>> {
    let mut decoded = Vec::new();

    for array_ref in &group.refs {
        let (element_offset, count, stride) = aref_read_params(array_ref);
        let raw = container.get_array_bytes_from_block(
            array_ref.block_id,
            element_offset,
            count,
            stride,
            "read_group_decoded_bytes",
        )?;
        let unfiltered = unfilter_array_bytes(raw, group.dtype, group.array_filter)?;
        decoded.extend_from_slice(&unfiltered);
    }

    Ok(decoded)
}

#[inline]
fn aref_read_params(array_ref: &ArrayRef) -> (u64, u64, usize) {
    if array_ref.encoded_len > 0 {
        (array_ref.element_offset, array_ref.encoded_len as u64, 1)
    } else {
        (
            array_ref.element_offset,
            array_ref.element_count,
            dtype_stride(array_ref.dtype),
        )
    }
}

fn array_byte_range(array_ref: &ArrayRef, ctx: &'static str) -> IonResult<(usize, usize)> {
    let (element_offset, count, stride) = aref_read_params(array_ref);
    let start = usize::try_from(element_offset)
        .ok()
        .and_then(|offset| offset.checked_mul(stride))
        .ok_or_else(|| {
            IonError::from(format!(
                "{ctx}: item range overflow for block {}",
                array_ref.block_id
            ))
        })?;
    let end = usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(stride))
        .and_then(|len| start.checked_add(len))
        .ok_or_else(|| {
            IonError::from(format!(
                "{ctx}: item range overflow for block {}",
                array_ref.block_id
            ))
        })?;
    Ok((start, end))
}

#[inline]
fn parse_array_pair(entry_bytes: &[u8], aref_bytes: &[u8]) -> Option<(ArrayRef, ArrayRef)> {
    let ref_start =
        usize::try_from(u64::from_le_bytes(entry_bytes[0..8].try_into().unwrap())).ok()?;
    let ref_count =
        usize::try_from(u64::from_le_bytes(entry_bytes[8..16].try_into().unwrap())).ok()?;
    let start = ref_start.checked_mul(ARRAY_REF_BYTES)?;
    let span = ref_count.checked_mul(ARRAY_REF_BYTES)?;
    let end = start.checked_add(span)?;
    let mut mz_ref = None;
    let mut int_ref = None;
    for bytes in aref_bytes.get(start..end)?.chunks_exact(ARRAY_REF_BYTES) {
        let array_ref = parse_array_ref(bytes);
        match array_ref.array_type {
            ACC_MZ => mz_ref = Some(array_ref),
            ACC_INT => int_ref = Some(array_ref),
            _ => {}
        }
        if let (Some(mz_ref), Some(int_ref)) = (mz_ref, int_ref) {
            return Some((mz_ref, int_ref));
        }
    }
    None
}

#[inline]
fn decode_from_block(
    container: &mut dyn ContainerAccess,
    buf: &mut Vec<f64>,
    array_ref: &ArrayRef,
) -> bool {
    let (element_offset, count, stride) = aref_read_params(array_ref);
    match container.get_array_bytes_from_block(
        array_ref.block_id,
        element_offset,
        count,
        stride,
        "scan",
    ) {
        Ok(raw) => decode_into(buf, raw, array_ref.dtype, array_ref.array_filter).is_ok(),
        Err(_) => false,
    }
}

#[inline]
fn dtype_stride(dtype: u8) -> usize {
    match dtype {
        FILE_DTYPE_F64 | FILE_DTYPE_I64 => 8,
        FILE_DTYPE_F32 | FILE_DTYPE_I32 => 4,
        FILE_DTYPE_F16 | FILE_DTYPE_I16 => 2,
        _ => 1,
    }
}

fn unfilter_array_bytes(
    raw: &[u8],
    dtype: u8,
    array_filter: u8,
) -> IonResult<std::borrow::Cow<'_, [u8]>> {
    let pk_id = PackingId::from_byte(array_filter)?;
    match pk_id {
        PackingId::Raw | PackingId::ByteShuffle => Ok(std::borrow::Cow::Borrowed(raw)),
        PackingId::DeltaShuffle => {
            if dtype == FILE_DTYPE_F64 {
                let mut out = Vec::with_capacity(raw.len());
                let mut prev: u64 = 0;
                for chunk in raw.chunks_exact(8) {
                    prev = prev.wrapping_add(u64::from_le_bytes(chunk.try_into().unwrap()));
                    out.extend_from_slice(&prev.to_le_bytes());
                }
                Ok(std::borrow::Cow::Owned(out))
            } else {
                Ok(std::borrow::Cow::Borrowed(raw))
            }
        }
    }
}

fn decode_into(buf: &mut Vec<f64>, raw: &[u8], dtype: u8, array_filter: u8) -> IonResult<()> {
    buf.clear();
    let bytes = unfilter_array_bytes(raw, dtype, array_filter)?;
    match dtype {
        FILE_DTYPE_F64 => {
            buf.reserve(bytes.len() / 8);
            buf.extend(
                bytes
                    .chunks_exact(8)
                    .map(|c| f64::from_le_bytes(c.try_into().unwrap())),
            );
        }
        FILE_DTYPE_F32 => {
            buf.reserve(bytes.len() / 4);
            buf.extend(
                bytes
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes(c.try_into().unwrap()) as f64),
            );
        }
        FILE_DTYPE_F16 => {
            buf.reserve(bytes.len() / 2);
            buf.extend(
                bytes
                    .chunks_exact(2)
                    .map(|c| f16_bits_to_f64(u16::from_le_bytes(c.try_into().unwrap()))),
            );
        }
        FILE_DTYPE_I16 => {
            buf.reserve(bytes.len() / 2);
            buf.extend(
                bytes
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes(c.try_into().unwrap()) as f64),
            );
        }
        FILE_DTYPE_I32 => {
            buf.reserve(bytes.len() / 4);
            buf.extend(
                bytes
                    .chunks_exact(4)
                    .map(|c| i32::from_le_bytes(c.try_into().unwrap()) as f64),
            );
        }
        FILE_DTYPE_I64 => {
            buf.reserve(bytes.len() / 8);
            buf.extend(
                bytes
                    .chunks_exact(8)
                    .map(|c| i64::from_le_bytes(c.try_into().unwrap()) as f64),
            );
        }
        _ => {
            return Err(IonError::BadDtype {
                dtype,
                kind: "decode array dtype",
            });
        }
    }
    Ok(())
}

fn attach_binaries<E: BinaryArrayOwner>(
    entries_buf: &[u8],
    arrayrefs_buf: &[u8],
    entries: &mut [E],
    container: &ContainerView<DefaultBlockProcessor>,
    ctx: &'static str,
    parallel: bool,
) -> IonResult<()> {
    let mut refs = Vec::new();
    let mut blocks = HashMap::new();
    #[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
    let _ = parallel;

    for index in 0..entries.len() {
        let Some(item_refs) = read_array_refs_from_buffers(entries_buf, arrayrefs_buf, index)
        else {
            continue;
        };
        if item_refs.is_empty() {
            continue;
        }
        for array_ref in item_refs.as_slice() {
            let stride = if array_ref.encoded_len > 0 {
                1
            } else {
                dtype_stride(array_ref.dtype)
            };
            if let Some(old) = blocks.insert(array_ref.block_id, stride)
                && old != stride
            {
                return Err(IonError::from(format!(
                    "{ctx}: stride mismatch for block {} (expected {old}, got {stride})",
                    array_ref.block_id
                )));
            }
        }
        refs.push((index, item_refs));
    }

    let mut block_list: Vec<_> = blocks.into_iter().collect();
    block_list.sort_unstable_by_key(|(block_id, _)| *block_id);

    let load = |(block_id, stride): (u32, usize)| -> IonResult<(u32, Vec<u8>)> {
        Ok((block_id, container.read_block(block_id, stride, ctx)?))
    };

    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    let data: HashMap<u32, Vec<u8>> = if parallel && block_list.len() >= 4 {
        block_list
            .into_par_iter()
            .map(load)
            .collect::<IonResult<Vec<_>>>()?
            .into_iter()
            .collect()
    } else {
        block_list
            .into_iter()
            .map(load)
            .collect::<IonResult<Vec<_>>>()?
            .into_iter()
            .collect()
    };
    #[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
    let data: HashMap<u32, Vec<u8>> = block_list
        .into_iter()
        .map(load)
        .collect::<IonResult<Vec<_>>>()?
        .into_iter()
        .collect();

    for (index, item_refs) in refs {
        let groups = group_arrays(item_refs.as_slice())?;
        let list = entries[index]
            .binary_data_array_list_mut()
            .get_or_insert_with(BinaryDataArrayList::default);

        for group in groups {
            let mut concatenated = Vec::new();

            for array_ref in &group.refs {
                let block = data.get(&array_ref.block_id).ok_or_else(|| {
                    IonError::from(format!("{ctx}: missing block {}", array_ref.block_id))
                })?;
                let (start, end) = {
                    let (element_offset, count, stride) = aref_read_params(array_ref);
                    let s = usize::try_from(element_offset)
                        .ok()
                        .and_then(|offset| offset.checked_mul(stride))
                        .ok_or_else(|| {
                            IonError::from(format!(
                                "{ctx}: item range overflow for block {}",
                                array_ref.block_id
                            ))
                        })?;
                    let e = usize::try_from(count)
                        .ok()
                        .and_then(|c| c.checked_mul(stride))
                        .and_then(|len| s.checked_add(len))
                        .ok_or_else(|| {
                            IonError::from(format!(
                                "{ctx}: item range overflow for block {}",
                                array_ref.block_id
                            ))
                        })?;
                    (s, e)
                };
                let raw = block.get(start..end).ok_or_else(|| {
                    IonError::from(format!(
                        "{ctx}: item range [{start}..{end}] out of bounds for block {} (len={})",
                        array_ref.block_id,
                        block.len()
                    ))
                })?;
                let unfiltered = unfilter_array_bytes(raw, group.dtype, group.array_filter)?;
                concatenated.extend_from_slice(&unfiltered);
            }

            attach_logical_array(list, group.array_type, group.dtype, &concatenated)?;
        }
        list.count = Some(list.binary_data_arrays.len());
    }

    Ok(())
}

fn attach_logical_array(
    binary_array_list: &mut BinaryDataArrayList,
    array_type: u32,
    dtype: u8,
    decoded_bytes: &[u8],
) -> IonResult<()> {
    let binary = decoded_bytes_to_binary_data(decoded_bytes, dtype)?;
    let numeric_type = dtype_to_numeric_type(dtype)?;

    let empty_index = binary_array_list
        .binary_data_arrays
        .iter()
        .position(|array| binary_array_has_type(array, array_type) && array.binary.is_none());

    let binary_array = if let Some(index) = empty_index {
        &mut binary_array_list.binary_data_arrays[index]
    } else {
        binary_array_list
            .binary_data_arrays
            .push(make_binary_array_stub(array_type));
        binary_array_list.binary_data_arrays.last_mut().unwrap()
    };

    binary_array.binary = Some(binary);
    sync_numeric_meta(binary_array, numeric_type);
    Ok(())
}

fn raw_to_vec<T>(raw: &[u8], elem_size: usize, read: impl Fn(&[u8]) -> T) -> IonResult<Vec<T>> {
    if raw.len() % elem_size != 0 {
        return Err(IonError::from(format!(
            "array: length {} not a multiple of {elem_size}",
            raw.len()
        )));
    }
    let mut out = Vec::with_capacity(raw.len() / elem_size);
    out.extend(raw.chunks_exact(elem_size).map(read));
    Ok(out)
}

fn decoded_bytes_to_binary_data(bytes: &[u8], dtype: u8) -> IonResult<BinaryData> {
    match dtype {
        FILE_DTYPE_F64 => Ok(BinaryData::F64(raw_to_vec(bytes, 8, |c| {
            f64::from_le_bytes(c.try_into().unwrap())
        })?)),
        FILE_DTYPE_F32 => Ok(BinaryData::F32(raw_to_vec(bytes, 4, |c| {
            f32::from_le_bytes(c.try_into().unwrap())
        })?)),
        FILE_DTYPE_F16 => Ok(BinaryData::F16(raw_to_vec(bytes, 2, |c| {
            u16::from_le_bytes(c.try_into().unwrap())
        })?)),
        FILE_DTYPE_I16 => Ok(BinaryData::I16(raw_to_vec(bytes, 2, |c| {
            i16::from_le_bytes(c.try_into().unwrap())
        })?)),
        FILE_DTYPE_I32 => Ok(BinaryData::I32(raw_to_vec(bytes, 4, |c| {
            i32::from_le_bytes(c.try_into().unwrap())
        })?)),
        FILE_DTYPE_I64 => Ok(BinaryData::I64(raw_to_vec(bytes, 8, |c| {
            i64::from_le_bytes(c.try_into().unwrap())
        })?)),
        _ => Err(IonError::from(format!(
            "unsupported dtype {dtype} in binary array"
        ))),
    }
}

fn dtype_to_numeric_type(dtype: u8) -> IonResult<NumericType> {
    match dtype {
        FILE_DTYPE_F64 => Ok(NumericType::Float64),
        FILE_DTYPE_F32 => Ok(NumericType::Float32),
        FILE_DTYPE_F16 => Ok(NumericType::Float16),
        FILE_DTYPE_I16 => Ok(NumericType::Int16),
        FILE_DTYPE_I32 => Ok(NumericType::Int32),
        FILE_DTYPE_I64 => Ok(NumericType::Int64),
        _ => Err(IonError::BadDtype {
            dtype,
            kind: "numeric type",
        }),
    }
}

fn make_binary_array_stub(array_type: u32) -> BinaryDataArray {
    BinaryDataArray {
        cv_params: vec![CvParam {
            cv_ref: Some("MS".to_string()),
            accession: Some(format_accession(array_type)),
            name: String::new(),
            ..Default::default()
        }],
        ..BinaryDataArray::default()
    }
}

#[inline]
fn sync_numeric_meta(binary_array: &mut BinaryDataArray, numeric_type: NumericType) {
    let target = match numeric_type {
        NumericType::Float16 => 1_000_520,
        NumericType::Float32 => 1_000_521,
        NumericType::Float64 => 1_000_523,
        NumericType::Int16 => 1_000_518,
        NumericType::Int32 => 1_000_519,
        NumericType::Int64 => 1_000_522,
    };
    binary_array
        .cv_params
        .retain(|param| !is_numeric_acc(parse_accession_tail(param.accession.as_deref())));
    binary_array.cv_params.push(CvParam {
        cv_ref: Some("MS".into()),
        accession: Some(format_accession(target)),
        name: match target {
            1_000_521 => "32-bit float",
            1_000_523 => "64-bit float",
            1_000_519 => "32-bit integer",
            1_000_522 => "64-bit integer",
            _ => "numeric",
        }
        .into(),
        ..Default::default()
    });
    binary_array.numeric_type = Some(numeric_type);
}

#[inline]
fn is_numeric_acc(tail: AccessionTail) -> bool {
    matches!(tail.raw(), 1_000_518..=1_000_523)
}

#[inline]
fn binary_array_has_type(binary_array: &BinaryDataArray, array_type: u32) -> bool {
    binary_array
        .cv_params
        .iter()
        .any(|param| parse_accession_tail(param.accession.as_deref()).raw() == array_type)
}

fn parse_run_source_file_refs(
    owner_rows: &OwnerRows,
    lookup: &ChildrenLookup,
    run_id: u32,
) -> Option<SourceFileRefList> {
    if let Some(&list_id) = lookup.ids_for(run_id, TagId::SourceFileRefList).first() {
        let refs: Vec<_> = lookup
            .ids_for(list_id, TagId::SourceFileRef)
            .iter()
            .filter_map(|&id| {
                get_attr_text(owner_rows.get(id), ACC_ATTR_REF)
                    .map(|value| SourceFileRef { r#ref: value })
            })
            .collect();
        if !refs.is_empty() {
            return Some(SourceFileRefList {
                count: Some(refs.len()),
                source_file_refs: refs,
            });
        }
    }

    if let Some(&list_id) = lookup.ids_for(run_id, TagId::SourceFileList).first() {
        let refs: Vec<_> = lookup
            .ids_for(list_id, TagId::SourceFile)
            .iter()
            .filter_map(|&id| {
                get_attr_text(owner_rows.get(id), ACC_ATTR_ID)
                    .map(|value| SourceFileRef { r#ref: value })
            })
            .collect();
        if !refs.is_empty() {
            return Some(SourceFileRefList {
                count: Some(refs.len()),
                source_file_refs: refs,
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ion::encoder::utilities::SectionChunkMode;
    use crate::mzml::structs::{BinaryData, BinaryDataArray, CvParam};

    const BYTES: &[u8] = include_bytes!("../../../data/ion/test.ion");

    fn ref_with(array_type: u32, continues_previous_segment: u8, encoded_len: u32) -> ArrayRef {
        ArrayRef {
            block_id: 0,
            element_offset: 0,
            element_count: 4,
            array_type,
            dtype: FILE_DTYPE_F64,
            array_filter: 0,
            encoded_len,
            continues_previous_segment,
        }
    }

    #[test]
    fn group_arrays_keeps_same_accession_logical_arrays_separate() {
        let refs = [ref_with(1000514, 0, 0), ref_with(1000514, 0, 0)];
        let groups = group_arrays(&refs).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].refs.len(), 1);
        assert_eq!(groups[1].refs.len(), 1);
    }

    #[test]
    fn group_arrays_joins_continuation_segments_into_one_group() {
        let refs = [ref_with(1000514, 0, 0), ref_with(1000514, 1, 0)];
        let groups = group_arrays(&refs).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].refs.len(), 2);
    }

    #[test]
    fn group_arrays_errors_on_leading_continuation() {
        let refs = [ref_with(1000514, 1, 0)];
        assert!(group_arrays(&refs).is_err());
    }

    #[test]
    fn group_arrays_errors_on_invalid_continues_value() {
        let refs = [ref_with(1000514, 0, 0), ref_with(1000514, 2, 0)];
        assert!(group_arrays(&refs).is_err());
    }

    #[test]
    fn group_arrays_errors_on_multi_ref_variable_length() {
        let refs = [ref_with(1000514, 0, 8), ref_with(1000514, 1, 8)];
        assert!(group_arrays(&refs).is_err());
    }

    #[test]
    fn group_arrays_errors_on_type_mismatch_in_continuation() {
        let mut second = ref_with(1000514, 1, 0);
        second.array_type = 1000515;
        let refs = [ref_with(1000514, 0, 0), second];
        assert!(group_arrays(&refs).is_err());
    }

    #[test]
    fn new_reader_opens_old_fixture() {
        let decoder = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        assert!(matches!(
            decoder.spec_segment_bounds,
            SegmentBoundsCache::Unloaded
        ));
    }

    #[test]
    fn spectrum_at_lazy_matches_full_conversion() {
        let mut decoder = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        let full = decoder.to_mzml().unwrap();
        let full_spectra = full.run.spectrum_list.expect("spectrum list").spectra;
        for index in 0..full_spectra.len() {
            let lazy = decoder
                .spectrum_at(index)
                .unwrap()
                .expect("spectrum present");
            assert_eq!(
                format!("{lazy:?}"),
                format!("{:?}", full_spectra[index]),
                "spectrum {index} differs between lazy and full paths"
            );
        }
    }

    #[test]
    fn metadata_at_matches_filtered_full_read() {
        let mut decoder = Decoder::open(BYTES, DecoderConfig::default()).unwrap();

        let all_spectra = decoder.spectrum_metadata().unwrap();
        for index in 0..decoder.spectrum_count() as usize {
            let one = decoder.spectrum_metadata_at(index).unwrap();
            let expected: Vec<_> = all_spectra
                .iter()
                .filter(|row| row.item_index as usize == index)
                .cloned()
                .collect();
            assert_eq!(format!("{one:?}"), format!("{expected:?}"));
        }

        let all_chroms = decoder.chromatogram_metadata().unwrap();
        for index in 0..decoder.chromatogram_count() as usize {
            let one = decoder.chromatogram_metadata_at(index).unwrap();
            let expected: Vec<_> = all_chroms
                .iter()
                .filter(|row| row.item_index as usize == index)
                .cloned()
                .collect();
            assert_eq!(format!("{one:?}"), format!("{expected:?}"));
        }
    }

    #[test]
    fn open_parses_header() {
        let d = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        assert!(d.spectrum_count() > 0);
    }

    #[test]
    fn summary_returns_none_out_of_bounds() {
        let d = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        assert!(d.spec_summary(d.spectrum_count() as usize).is_none());
    }

    #[test]
    fn summary_has_valid_rt() {
        let d = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        let r = d.spec_summary(0).unwrap();
        assert!(r.rt_seconds.is_finite() && r.rt_seconds >= 0.0);
        assert!(r.ms_level >= 1);
    }

    #[test]
    fn array_refs_contain_mz_and_intensity() {
        let d = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        let refs = d.spectrum_array_refs(0).unwrap();
        assert!(refs.iter().any(|a| a.array_type == ACC_MZ));
        assert!(refs.iter().any(|a| a.array_type == ACC_INT));
    }

    #[test]
    fn read_spectrum_array_produces_mz_values() {
        let mut d = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        let refs = d.spectrum_array_refs(0).unwrap();
        let mz_ref = refs.iter().find(|a| a.array_type == ACC_MZ).unwrap();

        let mut mz = Vec::new();
        d.read_spectrum_array(mz_ref, &mut mz).unwrap();

        assert!(!mz.is_empty());
        assert!(mz.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn for_each_scan_yields_matching_scans() {
        let mut d = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        let mut count = 0usize;
        d.for_each_in_range(0.0, f64::MAX, 0, |summary, mz, int| {
            assert!(summary.rt.is_finite());
            assert!(!mz.is_empty());
            assert_eq!(mz.len(), int.len());
            count += 1;
        });
        assert_eq!(count, d.spectrum_count() as usize);
    }

    #[test]
    fn for_each_scan_filters_by_ms_level() {
        let mut d = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        let mut count = 0usize;
        d.for_each_in_range(0.0, f64::MAX, 1, |_, _, _| {
            count += 1;
        });
        let expected = (0..d.spectrum_count() as usize)
            .filter(|&i| d.spec_summary(i).map_or(false, |r| r.ms_level == 1))
            .count();
        assert_eq!(count, expected);
    }

    #[allow(deprecated)]
    #[test]
    fn to_mzml_produces_valid_structure() {
        let mut d = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        let mzml = d.to_mzml().unwrap();
        let sl = mzml.run.spectrum_list.as_ref().unwrap();
        assert!(!sl.spectra.is_empty());
        assert!(
            sl.spectra[0]
                .binary_data_array_list
                .as_ref()
                .unwrap()
                .binary_data_arrays
                .iter()
                .any(|b| b.binary.is_some())
        );
    }

    #[test]
    fn global_metadata_returns_entries() {
        let d = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        assert!(!d.global_metadata().unwrap().is_empty());
    }

    #[test]
    fn spectrum_metadata_returns_entries() {
        let d = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        assert!(!d.spectrum_metadata().unwrap().is_empty());
    }

    #[test]
    fn custom_config_opens_successfully() {
        let config = DecoderConfig {
            max_cached_bytes: 1024 * 1024,
            verify_checksums: true,
            parallel: true,
            decompression_budget: DecompressionBudget::default(),
        };
        let d = Decoder::open(BYTES, config).unwrap();
        assert!(d.spectrum_count() > 0);
    }

    #[test]
    fn decode_into_f64_roundtrips() {
        let vals = [1.5f64, 2.5, 3.5];
        let raw: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut buf = Vec::new();
        decode_into(&mut buf, &raw, 1, 0).unwrap();
        assert_eq!(buf, vals);
    }

    #[test]
    fn decode_into_f32_converts() {
        let vals = [1.0f32, 2.0];
        let raw: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut buf = Vec::new();
        decode_into(&mut buf, &raw, 2, 0).unwrap();
        assert!((buf[0] - 1.0).abs() < f64::EPSILON);
        assert!((buf[1] - 2.0).abs() < f64::EPSILON);
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

    #[test]
    fn ion_open_has_metadata() {
        let mut ion = Ion::open(BYTES, DecoderConfig::default()).unwrap();
        ion.load_metadata().unwrap();
        assert!(ion.spectrum_count() > 0);
        assert!(ion.run.spectrum_list.is_some());
        assert!(ion.software_list.is_some() || ion.instrument_list.is_some());
    }

    #[test]
    fn ion_for_each_scan_yields_scans() {
        let mut ion = Ion::open(BYTES, DecoderConfig::default()).unwrap();
        let mut count = 0usize;
        ion.for_each_in_range(0.0, f64::MAX, 0, |summary, mz, int| {
            assert!(summary.rt.is_finite());
            assert!(!mz.is_empty());
            assert_eq!(mz.len(), int.len());
            count += 1;
        });
        assert_eq!(count, ion.spectrum_count() as usize);
    }

    #[test]
    fn ion_summary_matches_decoder() {
        let ion = Ion::open(BYTES, DecoderConfig::default()).unwrap();
        let r = ion.spec_summary(0).unwrap();
        assert!(r.rt_seconds.is_finite());
        assert!(r.ms_level >= 1);
    }

    #[test]
    fn ion_to_mzml_has_binaries() {
        let mut ion = Ion::open(BYTES, DecoderConfig::default()).unwrap();
        let mzml = ion.to_mzml().unwrap();
        let sl = mzml.run.spectrum_list.as_ref().unwrap();
        assert!(!sl.spectra.is_empty());
        assert!(
            sl.spectra[0]
                .binary_data_array_list
                .as_ref()
                .unwrap()
                .binary_data_arrays
                .iter()
                .any(|b| b.binary.is_some())
        );
    }

    #[test]
    fn open_arc_gives_same_result_as_open() {
        let bytes_arc: Arc<[u8]> = Arc::from(BYTES);
        let mut d1 = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        let mut d2 = Decoder::open_arc(bytes_arc, DecoderConfig::default()).unwrap();
        assert_eq!(d1.spectrum_count(), d2.spectrum_count());
        let mzml1 = d1.to_mzml().unwrap();
        let mzml2 = d2.to_mzml().unwrap();
        assert_eq!(format!("{mzml1:?}"), format!("{mzml2:?}"));
    }

    #[test]
    fn open_with_source_uses_provided_source() {
        use crate::ion::decoder::utilities::byte_source::SliceSource;
        let bytes_arc: Arc<[u8]> = Arc::from(BYTES);
        let source = Arc::new(SliceSource::new(bytes_arc.clone())) as Arc<dyn ByteSource>;
        let mut d = Decoder::open_with_source(source, DecoderConfig::default()).unwrap();
        assert!(d.spectrum_count() > 0);
        let mzml = d.to_mzml().unwrap();
        assert!(mzml.run.spectrum_list.unwrap().spectra.len() > 0);
    }

    #[test]
    fn mixed_normal_and_oversized_spectra_preserve_order_and_data() {
        use crate::ion::encoder::{
            encode::{EncodingConfig, TARGET_BLOCK_UNCOMPRESSED_BYTES},
            ion_writer::write_mzml_to_ion,
        };
        use crate::mzml::structs::{
            BinaryData, BinaryDataArray, BinaryDataArrayList, CvParam, MzML, Run, Spectrum,
            SpectrumList,
        };

        fn make_bda(accession: &str, name: &str, data: Vec<f64>) -> BinaryDataArray {
            BinaryDataArray {
                cv_params: vec![CvParam {
                    cv_ref: Some("MS".to_string()),
                    accession: Some(accession.to_string()),
                    name: name.to_string(),
                    value: None,
                    unit_cv_ref: None,
                    unit_name: None,
                    unit_accession: None,
                }],
                binary: Some(BinaryData::F64(data)),
                ..Default::default()
            }
        }

        fn make_spectrum(id: &str, mz: Vec<f64>, int: Vec<f64>) -> Spectrum {
            Spectrum {
                id: id.to_string(),
                binary_data_array_list: Some(BinaryDataArrayList {
                    count: Some(2),
                    binary_data_arrays: vec![
                        make_bda("MS:1000514", "m/z array", mz),
                        make_bda("MS:1000515", "intensity array", int),
                    ],
                }),
                ..Default::default()
            }
        }

        let small_count_before = 5;
        let small_count_after = 5;
        let huge_n = (TARGET_BLOCK_UNCOMPRESSED_BYTES / 8) * 2;

        let mut expected_spectra: Vec<(String, Vec<f64>, Vec<f64>)> = Vec::new();
        let mut spectra = Vec::new();

        for i in 0..small_count_before {
            let mz: Vec<f64> = (0..10).map(|j| 100.0 + i as f64 + j as f64 * 0.1).collect();
            let int: Vec<f64> = (0..10).map(|j| (i * 10 + j) as f64).collect();
            let id = format!("small_pre_{i}");
            spectra.push(make_spectrum(&id, mz.clone(), int.clone()));
            expected_spectra.push((id, mz, int));
        }

        let huge_mz: Vec<f64> = (0..huge_n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let huge_int: Vec<f64> = (0..huge_n).map(|i| i as f64 * 10.0).collect();
        let huge_id = "huge_ms1".to_string();
        spectra.push(make_spectrum(&huge_id, huge_mz.clone(), huge_int.clone()));
        expected_spectra.push((huge_id, huge_mz, huge_int));

        for i in 0..small_count_after {
            let mz: Vec<f64> = (0..10).map(|j| 500.0 + i as f64 + j as f64 * 0.1).collect();
            let int: Vec<f64> = (0..10).map(|j| (i * 20 + j) as f64).collect();
            let id = format!("small_post_{i}");
            spectra.push(make_spectrum(&id, mz.clone(), int.clone()));
            expected_spectra.push((id, mz, int));
        }

        let mzml_in = MzML {
            run: Run {
                spectrum_list: Some(SpectrumList {
                    spectra,
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut encoded = Vec::new();
        write_mzml_to_ion(
            &mzml_in,
            EncodingConfig {
                compression_level: 3,
                force_f32: false,
                uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
                parallel: true,
                section_chunk: SectionChunkMode::Memory,
                target_segment_bytes: 128 * 1024,
                min_split_bytes: 512 * 1024,
            },
            &mut encoded,
        )
        .unwrap();

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        let mzml_out = decoder.to_mzml().unwrap();
        let out_spectra = mzml_out.run.spectrum_list.unwrap().spectra;

        assert_eq!(out_spectra.len(), expected_spectra.len());

        for (out, (expected_id, expected_mz, expected_int)) in
            out_spectra.iter().zip(expected_spectra.iter())
        {
            assert_eq!(&out.id, expected_id);
            let arrays = out
                .binary_data_array_list
                .as_ref()
                .unwrap()
                .binary_data_arrays
                .as_slice();
            let mz_out = arrays
                .iter()
                .find(|a| {
                    a.cv_params
                        .iter()
                        .any(|cv| cv.accession.as_deref() == Some("MS:1000514"))
                })
                .and_then(|a| a.binary.as_ref())
                .unwrap();
            let int_out = arrays
                .iter()
                .find(|a| {
                    a.cv_params
                        .iter()
                        .any(|cv| cv.accession.as_deref() == Some("MS:1000515"))
                })
                .and_then(|a| a.binary.as_ref())
                .unwrap();
            let BinaryData::F64(mz_vec) = mz_out else {
                panic!("expected F64 mz array for {expected_id}");
            };
            let BinaryData::F64(int_vec) = int_out else {
                panic!("expected F64 intensity array for {expected_id}");
            };
            assert_eq!(mz_vec, expected_mz, "mz mismatch for {expected_id}");
            assert_eq!(
                int_vec, expected_int,
                "intensity mismatch for {expected_id}"
            );
        }
    }

    #[test]
    fn oversized_array_roundtrips_with_compression_and_parallel() {
        use crate::ion::encoder::{
            encode::{EncodingConfig, TARGET_BLOCK_UNCOMPRESSED_BYTES},
            ion_writer::write_mzml_to_ion,
        };
        use crate::mzml::structs::{
            BinaryData, BinaryDataArray, BinaryDataArrayList, CvParam, MzML, Run, Spectrum,
            SpectrumList,
        };

        let n = (TARGET_BLOCK_UNCOMPRESSED_BYTES / 8) * 2;
        let mz_data: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int_data: Vec<f64> = (0..n).map(|i| i as f64 * 10.0).collect();

        fn make_bda(accession: &str, name: &str, data: Vec<f64>) -> BinaryDataArray {
            BinaryDataArray {
                cv_params: vec![CvParam {
                    cv_ref: Some("MS".to_string()),
                    accession: Some(accession.to_string()),
                    name: name.to_string(),
                    value: None,
                    unit_cv_ref: None,
                    unit_name: None,
                    unit_accession: None,
                }],
                binary: Some(BinaryData::F64(data)),
                ..Default::default()
            }
        }

        let spectrum = Spectrum {
            id: "scan=1".to_string(),
            binary_data_array_list: Some(BinaryDataArrayList {
                count: Some(2),
                binary_data_arrays: vec![
                    make_bda("MS:1000514", "m/z array", mz_data.clone()),
                    make_bda("MS:1000515", "intensity array", int_data.clone()),
                ],
            }),
            ..Default::default()
        };

        let mzml_in = MzML {
            run: Run {
                spectrum_list: Some(SpectrumList {
                    spectra: vec![spectrum],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut encoded = Vec::new();
        write_mzml_to_ion(
            &mzml_in,
            EncodingConfig {
                compression_level: 3,
                force_f32: false,
                uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
                parallel: true,
                section_chunk: SectionChunkMode::Memory,
                target_segment_bytes: 128 * 1024,
                min_split_bytes: 512 * 1024,
            },
            &mut encoded,
        )
        .unwrap();

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        let mzml_out = decoder.to_mzml().unwrap();

        let spectra = mzml_out.run.spectrum_list.unwrap().spectra;
        let arrays = spectra[0]
            .binary_data_array_list
            .as_ref()
            .unwrap()
            .binary_data_arrays
            .as_slice();

        let mz_out = arrays
            .iter()
            .find(|a| {
                a.cv_params
                    .iter()
                    .any(|cv| cv.accession.as_deref() == Some("MS:1000514"))
            })
            .and_then(|a| a.binary.as_ref())
            .unwrap();
        let int_out = arrays
            .iter()
            .find(|a| {
                a.cv_params
                    .iter()
                    .any(|cv| cv.accession.as_deref() == Some("MS:1000515"))
            })
            .and_then(|a| a.binary.as_ref())
            .unwrap();

        let BinaryData::F64(mz_vec) = mz_out else {
            panic!("expected F64 mz array");
        };
        let BinaryData::F64(int_vec) = int_out else {
            panic!("expected F64 intensity array");
        };

        assert_eq!(mz_vec.len(), n);
        assert_eq!(int_vec.len(), n);
        assert_eq!(mz_vec, &mz_data);
        assert_eq!(int_vec, &int_data);
    }

    #[test]
    fn oversized_array_roundtrips_through_encode_decode() {
        use crate::ion::encoder::{
            encode::{EncodingConfig, TARGET_BLOCK_UNCOMPRESSED_BYTES},
            ion_writer::write_mzml_to_ion,
        };
        use crate::mzml::structs::{
            BinaryData, BinaryDataArray, BinaryDataArrayList, CvParam, MzML, Run, Spectrum,
            SpectrumList,
        };

        let n = (TARGET_BLOCK_UNCOMPRESSED_BYTES / 8) * 2;
        let mz_data: Vec<f64> = (0..n).map(|i| i as f64 * 0.001).collect();
        let int_data: Vec<f64> = (0..n).map(|i| i as f64 * 10.0).collect();

        fn make_bda(accession: &str, name: &str, data: Vec<f64>) -> BinaryDataArray {
            BinaryDataArray {
                cv_params: vec![CvParam {
                    cv_ref: Some("MS".to_string()),
                    accession: Some(accession.to_string()),
                    name: name.to_string(),
                    value: None,
                    unit_cv_ref: None,
                    unit_name: None,
                    unit_accession: None,
                }],
                binary: Some(BinaryData::F64(data)),
                ..Default::default()
            }
        }

        let spectrum = Spectrum {
            id: "scan=1".to_string(),
            binary_data_array_list: Some(BinaryDataArrayList {
                count: Some(2),
                binary_data_arrays: vec![
                    make_bda("MS:1000514", "m/z array", mz_data.clone()),
                    make_bda("MS:1000515", "intensity array", int_data.clone()),
                ],
            }),
            ..Default::default()
        };

        let mzml_in = MzML {
            run: Run {
                spectrum_list: Some(SpectrumList {
                    spectra: vec![spectrum],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut encoded = Vec::new();
        write_mzml_to_ion(
            &mzml_in,
            EncodingConfig {
                compression_level: 0,
                force_f32: false,
                uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
                parallel: false,
                section_chunk: SectionChunkMode::Memory,
                target_segment_bytes: 128 * 1024,
                min_split_bytes: 512 * 1024,
            },
            &mut encoded,
        )
        .unwrap();

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        let mzml_out = decoder.to_mzml().unwrap();

        let spectra = mzml_out.run.spectrum_list.unwrap().spectra;
        let arrays = spectra[0]
            .binary_data_array_list
            .as_ref()
            .unwrap()
            .binary_data_arrays
            .as_slice();

        let mz_out = arrays
            .iter()
            .find(|a| {
                a.cv_params
                    .iter()
                    .any(|cv| cv.accession.as_deref() == Some("MS:1000514"))
            })
            .and_then(|a| a.binary.as_ref())
            .unwrap();
        let int_out = arrays
            .iter()
            .find(|a| {
                a.cv_params
                    .iter()
                    .any(|cv| cv.accession.as_deref() == Some("MS:1000515"))
            })
            .and_then(|a| a.binary.as_ref())
            .unwrap();

        let BinaryData::F64(mz_vec) = mz_out else {
            panic!("expected F64 mz array");
        };
        let BinaryData::F64(int_vec) = int_out else {
            panic!("expected F64 intensity array");
        };

        assert_eq!(mz_vec.len(), n);
        assert_eq!(int_vec.len(), n);
        assert_eq!(mz_vec, &mz_data);
        assert_eq!(int_vec, &int_data);
    }

    fn make_split_bda(accession: &str, name: &str, data: Vec<f64>) -> BinaryDataArray {
        BinaryDataArray {
            cv_params: vec![CvParam {
                cv_ref: Some("MS".to_string()),
                accession: Some(accession.to_string()),
                name: name.to_string(),
                value: None,
                unit_cv_ref: None,
                unit_name: None,
                unit_accession: None,
            }],
            binary: Some(BinaryData::F64(data)),
            ..Default::default()
        }
    }

    fn encode_one_spectrum_with_split(
        mz: Vec<f64>,
        int: Vec<f64>,
        target_segment_bytes: usize,
        min_split_bytes: usize,
    ) -> Vec<u8> {
        encode_one_spectrum_with_split_mode(
            mz,
            int,
            target_segment_bytes,
            min_split_bytes,
            SectionChunkMode::Memory,
        )
    }

    fn encode_one_spectrum_with_split_mode(
        mz: Vec<f64>,
        int: Vec<f64>,
        target_segment_bytes: usize,
        min_split_bytes: usize,
        mode: SectionChunkMode,
    ) -> Vec<u8> {
        use crate::ion::encoder::{
            encode::{EncodingConfig, TARGET_BLOCK_UNCOMPRESSED_BYTES},
            ion_writer::write_mzml_to_ion,
        };
        use crate::mzml::structs::{BinaryDataArrayList, MzML, Run, Spectrum, SpectrumList};

        let spectrum = Spectrum {
            id: "split_ms1".to_string(),
            binary_data_array_list: Some(BinaryDataArrayList {
                count: Some(2),
                binary_data_arrays: vec![
                    make_split_bda("MS:1000514", "m/z array", mz),
                    make_split_bda("MS:1000515", "intensity array", int),
                ],
            }),
            ..Default::default()
        };

        let mzml_in = MzML {
            run: Run {
                spectrum_list: Some(SpectrumList {
                    spectra: vec![spectrum],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut encoded = Vec::new();
        write_mzml_to_ion(
            &mzml_in,
            EncodingConfig {
                compression_level: 3,
                force_f32: false,
                uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
                parallel: true,
                section_chunk: mode,
                target_segment_bytes,
                min_split_bytes,
            },
            &mut encoded,
        )
        .unwrap();
        encoded
    }

    #[test]
    fn split_mz_array_roundtrips_through_to_mzml() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| (i % 1000) as f64).collect();

        let encoded =
            encode_one_spectrum_with_split(mz.clone(), int.clone(), 64 * 1024, 128 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        assert!(
            decoder.header.spec_block_count >= 4,
            "splitting must produce several segment blocks, got {}",
            decoder.header.spec_block_count
        );

        let mzml_out = decoder.to_mzml().unwrap();
        let spectra = mzml_out.run.spectrum_list.unwrap().spectra;
        assert_eq!(spectra.len(), 1);

        let arrays = &spectra[0]
            .binary_data_array_list
            .as_ref()
            .unwrap()
            .binary_data_arrays;
        let mz_arrays: Vec<_> = arrays
            .iter()
            .filter(|a| {
                a.cv_params
                    .iter()
                    .any(|cv| cv.accession.as_deref() == Some("MS:1000514"))
            })
            .collect();
        assert_eq!(
            mz_arrays.len(),
            1,
            "split segments must reconstruct one logical m/z array"
        );

        let BinaryData::F64(mz_out) = mz_arrays[0].binary.as_ref().unwrap() else {
            panic!("expected F64 mz");
        };
        assert_eq!(mz_out, &mz);
    }

    #[test]
    fn split_mz_array_roundtrips_through_spectrum_at() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| (i % 777) as f64).collect();

        let encoded =
            encode_one_spectrum_with_split(mz.clone(), int.clone(), 64 * 1024, 128 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        let spectrum = decoder.spectrum_at(0).unwrap().unwrap();
        let arrays = &spectrum
            .binary_data_array_list
            .as_ref()
            .unwrap()
            .binary_data_arrays;

        let mz_arrays: Vec<_> = arrays
            .iter()
            .filter(|a| {
                a.cv_params
                    .iter()
                    .any(|cv| cv.accession.as_deref() == Some("MS:1000514"))
            })
            .collect();
        assert_eq!(mz_arrays.len(), 1);
        let BinaryData::F64(mz_out) = mz_arrays[0].binary.as_ref().unwrap() else {
            panic!("expected F64 mz");
        };
        assert_eq!(mz_out, &mz);

        let int_arrays: Vec<_> = arrays
            .iter()
            .filter(|a| {
                a.cv_params
                    .iter()
                    .any(|cv| cv.accession.as_deref() == Some("MS:1000515"))
            })
            .collect();
        assert_eq!(int_arrays.len(), 1);
        let BinaryData::F64(int_out) = int_arrays[0].binary.as_ref().unwrap() else {
            panic!("expected F64 intensity");
        };
        assert_eq!(int_out, &int);
    }

    #[test]
    fn read_spectrum_logical_array_joins_split_segments() {
        let n = 40_000;
        let mz: Vec<f64> = (0..n).map(|i| 200.0 + i as f64 * 0.002).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();

        let encoded = encode_one_spectrum_with_split(mz.clone(), int.clone(), 32 * 1024, 64 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        let mz_out = decoder
            .read_spectrum_logical_array(0, crate::accessions::MZ_ARRAY)
            .unwrap();
        assert_eq!(mz_out, mz);
    }

    #[test]
    fn centroided_small_arrays_are_encoded_correctly() {
        let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
        let int: Vec<f64> = (0..10).map(|i| i as f64).collect();

        let encoded = encode_one_spectrum_with_split(mz, int, 128 * 1024, 512 * 1024);

        let decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
    }

    #[test]
    fn split_mz_array_roundtrips_with_disk_staged_bounds() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| (i % 1000) as f64).collect();

        let encoded = encode_one_spectrum_with_split_mode(
            mz.clone(),
            int.clone(),
            64 * 1024,
            128 * 1024,
            SectionChunkMode::Disk,
        );

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();

        let mz_out = decoder
            .read_spectrum_logical_array(0, crate::accessions::MZ_ARRAY)
            .unwrap();
        assert_eq!(mz_out, mz);
    }

    fn brute_force_window(mz: &[f64], int: &[f64], low: f64, high: f64) -> (Vec<f64>, Vec<f64>) {
        let mut mz_out = Vec::new();
        let mut int_out = Vec::new();
        for (index, &value) in mz.iter().enumerate() {
            if value >= low && value <= high {
                mz_out.push(value);
                int_out.push(int[index]);
            }
        }
        (mz_out, int_out)
    }

    #[test]
    fn window_fast_path_matches_brute_force() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded =
            encode_one_spectrum_with_split(mz.clone(), int.clone(), 64 * 1024, 128 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();

        let windows = [
            (120.0, 130.0),
            (100.0, 149.999),
            (100.0001, 100.0009),
            (130.5, 130.5),
            (200.0, 300.0),
            (0.0, 50.0),
        ];
        for (low, high) in windows {
            let got = decoder.read_spectrum_mz_window(0, low, high).unwrap();
            let (expected_mz, expected_int) = brute_force_window(&mz, &int, low, high);
            assert_eq!(got.mz, expected_mz, "mz mismatch for window {low}..{high}");
            assert_eq!(
                got.intensity, expected_int,
                "intensity mismatch for window {low}..{high}"
            );
        }

        assert!(
            matches!(decoder.spec_segment_bounds, SegmentBoundsCache::Loaded(_)),
            "fast path should have loaded A3"
        );
    }

    #[test]
    fn window_fallback_matches_fast_path_on_split_file() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded =
            encode_one_spectrum_with_split(mz.clone(), int.clone(), 64 * 1024, 128 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        decoder.spec_segment_bounds = SegmentBoundsCache::Absent;

        for (low, high) in [(120.0, 130.0), (100.0, 149.999), (130.5, 130.5)] {
            let got = decoder.read_spectrum_mz_window(0, low, high).unwrap();
            let (expected_mz, expected_int) = brute_force_window(&mz, &int, low, high);
            assert_eq!(got.mz, expected_mz);
            assert_eq!(got.intensity, expected_int);
        }
    }

    #[test]
    fn window_on_unsplit_array_uses_fallback_and_is_correct() {
        let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
        let int: Vec<f64> = (0..10).map(|i| (i * 7) as f64).collect();
        let encoded =
            encode_one_spectrum_with_split(mz.clone(), int.clone(), 128 * 1024, 512 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();

        let got = decoder.read_spectrum_mz_window(0, 102.0, 105.0).unwrap();
        let (expected_mz, expected_int) = brute_force_window(&mz, &int, 102.0, 105.0);
        assert_eq!(got.mz, expected_mz);
        assert_eq!(got.intensity, expected_int);
    }

    #[test]
    fn window_empty_when_low_above_high() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 64 * 1024, 128 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        let got = decoder.read_spectrum_mz_window(0, 130.0, 120.0).unwrap();
        assert!(got.mz.is_empty());
        assert!(got.intensity.is_empty());
    }

    #[test]
    fn window_inverted_range_inside_data_unsplit_does_not_panic() {
        let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
        let int: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 128 * 1024, 512 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        let got = decoder.read_spectrum_mz_window(0, 105.0, 102.0).unwrap();
        assert!(got.mz.is_empty());
        assert!(got.intensity.is_empty());
    }

    #[test]
    fn window_out_of_range_index_errors() {
        let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
        let int: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 128 * 1024, 512 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        assert!(decoder.read_spectrum_mz_window(5, 100.0, 200.0).is_err());
    }

    #[test]
    fn window_forwards_through_ion() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded =
            encode_one_spectrum_with_split(mz.clone(), int.clone(), 64 * 1024, 128 * 1024);

        let bytes_arc: Arc<[u8]> = Arc::from(encoded.as_slice());
        let mut ion = Ion::open_arc(bytes_arc, DecoderConfig::default()).unwrap();
        let got = ion.read_spectrum_mz_window(0, 120.0, 130.0).unwrap();
        let (expected_mz, expected_int) = brute_force_window(&mz, &int, 120.0, 130.0);
        assert_eq!(got.mz, expected_mz);
        assert_eq!(got.intensity, expected_int);
    }

    #[test]
    fn a3_is_sparse_rows_for_mz_segments_none_for_intensity() {
        use crate::ion::axes::{Axis, axis_of};

        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 64 * 1024, 128 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        let _ = decoder.read_spectrum_mz_window(0, 120.0, 130.0).unwrap();
        let SegmentBoundsCache::Loaded(index) = &decoder.spec_segment_bounds else {
            panic!("A3 should be loaded");
        };

        let array_refs =
            read_array_refs_from_buffers(&decoder.spec_entries_buf, &decoder.spec_array_refs, 0)
                .unwrap();
        let groups = group_arrays(array_refs.as_slice()).unwrap();

        let mut position = 0u64;
        let mut mz_range = None;
        let mut intensity_range = None;
        for group in &groups {
            let count = group.refs.len() as u64;
            if matches!(axis_of(group.array_type), Some(Axis::Mz)) {
                mz_range = Some((position, count));
            } else if group.array_type == ACC_INT {
                intensity_range = Some((position, count));
            }
            position += count;
        }

        let (mz_base, mz_count) = mz_range.unwrap();
        let (intensity_base, intensity_count) = intensity_range.unwrap();
        assert!(mz_count >= 2, "expected the m/z array to be split");
        assert_eq!(mz_count, intensity_count);

        for i in 0..mz_count {
            assert!(
                index.get(mz_base + i).is_some(),
                "missing A3 row for m.z segment {i}"
            );
        }
        for i in 0..intensity_count {
            assert!(
                index.get(intensity_base + i).is_none(),
                "intensity segment {i} must not have an A3 row"
            );
        }
    }

    #[test]
    fn window_on_non_monotonic_mz_emits_no_a3_and_scans_correctly() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + (i % 1000) as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded =
            encode_one_spectrum_with_split(mz.clone(), int.clone(), 64 * 1024, 128 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();

        for (low, high) in [(100.2, 100.5), (100.0, 100.999), (100.45, 100.45)] {
            let got = decoder.read_spectrum_mz_window(0, low, high).unwrap();
            let (expected_mz, expected_int) = brute_force_window(&mz, &int, low, high);
            assert_eq!(got.mz, expected_mz, "mz mismatch for window {low}..{high}");
            assert_eq!(
                got.intensity, expected_int,
                "intensity mismatch for window {low}..{high}"
            );
        }
    }

    #[test]
    fn window_corrupt_a3_checksum_falls_back_correctly() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let mut encoded =
            encode_one_spectrum_with_split(mz.clone(), int.clone(), 64 * 1024, 128 * 1024);

        let a3_offset = {
            let header = parse_header(&encoded[..1024]).unwrap();
            header.off_spec_segment_bounds as usize
        };
        if a3_offset > 0 {
            encoded[a3_offset] ^= 0xFF;
        }

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        let got = decoder.read_spectrum_mz_window(0, 120.0, 130.0).unwrap();
        let (expected_mz, expected_int) = brute_force_window(&mz, &int, 120.0, 130.0);
        assert_eq!(got.mz, expected_mz);
        assert_eq!(got.intensity, expected_int);
        assert!(matches!(
            decoder.spec_segment_bounds,
            SegmentBoundsCache::Absent
        ));
    }

    #[test]
    fn generic_window_matches_mz_wrapper() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 64 * 1024, 128 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();

        let generic = decoder
            .read_spectrum_window(0, crate::accessions::MZ_ARRAY, ACC_INT, 120.0, 130.0)
            .unwrap();

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
        let mz_window = decoder.read_spectrum_mz_window(0, 120.0, 130.0).unwrap();

        assert_eq!(generic.x, mz_window.mz);
        assert_eq!(generic.y, mz_window.intensity);
    }

    #[test]
    fn generic_window_missing_accession_returns_empty() {
        let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
        let int: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 128 * 1024, 512 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();

        let got = decoder
            .read_spectrum_window(0, 99_999_999, ACC_INT, 1.0, 2.0)
            .unwrap();

        assert!(got.x.is_empty());
        assert!(got.y.is_empty());
    }

    fn spec_directory_range(header: &Header) -> (usize, usize) {
        let entry_size =
            crate::ion::encoder::utilities::container_builder::BLOCK_DIRECTORY_ENTRY_SIZE as u64;
        let directory_size = header.spec_block_count * entry_size;
        let end = header.off_spec_container + header.len_spec_container;
        let start = end - directory_size;
        (start as usize, end as usize)
    }

    #[test]
    fn directory_crc_roundtrips() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 64 * 1024, 128 * 1024);

        let header = parse_header(&encoded[..1024]).unwrap();
        let (start, end) = spec_directory_range(&header);
        let computed = crc32fast::hash(&encoded[start..end]);

        assert_eq!(computed, header.spec_directory_crc32);
        assert!(Decoder::open(&encoded, DecoderConfig::default()).is_ok());
    }

    #[test]
    fn flipped_directory_offset_is_caught_before_any_read() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let mut encoded = encode_one_spectrum_with_split(mz, int, 64 * 1024, 128 * 1024);

        let header = parse_header(&encoded[..1024]).unwrap();
        let (start, _end) = spec_directory_range(&header);
        encoded[start] ^= 0xFF;

        let result = Decoder::open(&encoded, DecoderConfig::default());
        assert!(result.is_err());
        let message = format!("{}", result.err().unwrap());
        assert!(message.contains("directory checksum mismatch"));
    }

    #[test]
    fn verify_off_skips_directory_check() {
        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let mut encoded = encode_one_spectrum_with_split(mz, int, 64 * 1024, 128 * 1024);

        let header = parse_header(&encoded[..1024]).unwrap();
        let (start, _end) = spec_directory_range(&header);
        encoded[start] ^= 0xFF;

        let config = DecoderConfig {
            verify_checksums: false,
            ..DecoderConfig::default()
        };
        assert!(Decoder::open(&encoded, config).is_ok());
    }

    #[test]
    fn empty_container_directory_crc_is_consistent() {
        let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
        let int: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 128 * 1024, 512 * 1024);

        let header = parse_header(&encoded[..1024]).unwrap();
        assert_eq!(header.chrom_block_count, 0);
        assert_eq!(header.chrom_directory_crc32, crc32fast::hash(&[]));
        assert!(Decoder::open(&encoded, DecoderConfig::default()).is_ok());
    }

    #[test]
    fn a3_b3_are_core_sections() {
        let mz: Vec<f64> = (0..1000).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..1000).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 64 * 1024, 128 * 1024);

        let header = parse_header(&encoded[..1024]).unwrap();

        assert!(
            header.len_spec_segment_bounds > 0,
            "A3 should be written as core section"
        );
    }

    #[test]
    fn candidate_items_filters_by_axis_accession() {
        use crate::accessions::INTENSITY_ARRAY;

        let n = 50_000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 64 * 1024, 128 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();

        let mz_candidates = decoder
            .candidate_items_for_axis(Target::Spec, crate::accessions::MZ_ARRAY, 120.0, 130.0)
            .unwrap();
        let int_candidates = decoder
            .candidate_items_for_axis(Target::Spec, INTENSITY_ARRAY, 1000.0, 2000.0)
            .unwrap();

        assert!(
            !mz_candidates.is_empty(),
            "m/z candidates should be found with bounds"
        );
        assert!(
            int_candidates.is_empty(),
            "intensity is not an axis, so no candidates"
        );
    }

    #[test]
    fn candidate_items_falls_back_when_bounds_missing() {
        let n = 1000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 64 * 1024, 128 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();

        decoder.spec_segment_bounds = SegmentBoundsCache::Absent;

        let candidates = decoder
            .candidate_items_for_axis(Target::Spec, crate::accessions::MZ_ARRAY, 120.0, 130.0)
            .unwrap();

        assert!(
            !candidates.is_empty(),
            "should return candidates when bounds unavailable (fallback)"
        );
    }

    #[test]
    fn core_a3_crc_failure_falls_back() {
        let n = 5000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let mut encoded =
            encode_one_spectrum_with_split(mz.clone(), int.clone(), 64 * 1024, 128 * 1024);

        let a3_offset = {
            let header = parse_header(&encoded[..1024]).unwrap();
            header.off_spec_segment_bounds as usize
        };

        if a3_offset > 0 {
            encoded[a3_offset] ^= 0xFF;

            let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();
            let candidates = decoder
                .candidate_items_for_axis(Target::Spec, crate::accessions::MZ_ARRAY, 120.0, 130.0)
                .unwrap();

            assert!(
                !candidates.is_empty(),
                "should return all candidates when A3 CRC fails"
            );
            assert!(
                matches!(decoder.spec_segment_bounds, SegmentBoundsCache::Absent),
                "A3 should be marked absent after CRC failure"
            );
        }
    }

    #[test]
    fn b3_header_fields_are_populated() {
        let n = 5000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 64 * 1024, 128 * 1024);

        let header = parse_header(&encoded[..1024]).unwrap();

        assert_eq!(
            header.off_chrom_segment_bounds, 0,
            "B3 offset should be 0 (no chromatograms)"
        );
        assert_eq!(
            header.len_chrom_segment_bounds, 0,
            "B3 length should be 0 (no chromatograms)"
        );
        assert_eq!(
            header.plain_len_chrom_segment_bounds, 0,
            "B3 plain_len should be 0 (no chromatograms)"
        );
    }

    #[test]
    fn candidate_items_for_chrom_axis_without_bounds() {
        let n = 1000;
        let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
        let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let encoded = encode_one_spectrum_with_split(mz, int, 128 * 1024, 512 * 1024);

        let mut decoder = Decoder::open(&encoded, DecoderConfig::default()).unwrap();

        let candidates = decoder
            .candidate_items_for_axis(Target::Chrom, crate::accessions::TIME_ARRAY, 0.0, 1000.0)
            .unwrap();

        assert!(
            candidates.is_empty(),
            "chromatogram query should return empty when file has no chromatograms"
        );
    }
}
