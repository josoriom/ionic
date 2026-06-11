use std::sync::Arc;

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use crate::ion::decoder::utilities::byte_source::FileSource;

use crate::{
    accessions::{
        FLOAT_16BIT, FLOAT_32BIT, FLOAT_64BIT, INT_16BIT, INT_32BIT, INT_64BIT, format_accession,
    },
    encoder::encode::{CHROM_SUMMARY_SIZE, SPEC_SUMMARY_SIZE},
    ion::{
        IonError, IonResult, Range,
        attr_meta::{
            ACC_ATTR_DEFAULT_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_DEFAULT_SOURCE_FILE_REF,
            ACC_ATTR_ID, ACC_ATTR_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_REF, ACC_ATTR_SAMPLE_REF,
            ACC_ATTR_START_TIME_STAMP, AccessionTail, parse_accession_tail,
        },
        decoder::utilities::byte_source::{
            BytesSource, CallbackSource, ReadBytes,
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
            block_reader::{
                BlockReader, ContainerAccess, DefaultBlockProcessor, container_directory_range,
            },
            decompression_limit::DecompressionLimit,
            parse_chromatogram_list, parse_cv_and_user_params, parse_cv_list,
            parse_data_processing_list, parse_file_description,
            parse_global_metadata::parse_global_metadata,
            Header, parse_header,
            parse_instrument_list, parse_referenceable_param_group_list, parse_sample_list,
            parse_scan_settings_list, parse_software_list, parse_spectrum, parse_spectrum_list,
            segment_bounds::SegmentBoundsIndex,
            spectrum_source::{
                ScanSource, ScanSummary, f16_bits_to_f64,
            },
        },
    },
    mzml::{schema::TagId, structs::*},
};

pub(crate) const ACC_MZ: u32 = 1_000_514;
pub(crate) const ACC_INT: u32 = 1_000_515;
pub(crate) const INDEX_ENTRY_BYTES: usize = 16;
pub(crate) const ARRAY_REF_BYTES: usize = 32;
const DEFAULT_MAX_CACHED_BYTES: usize = 256 * 1024 * 1024;
pub(crate) const INLINE_ARRAY_REF_CAP: usize = 8;

fn check_crc(bytes: &[u8], expected: u32, name: &str) -> IonResult<()> {
    let computed = crc32fast::hash(bytes);
    if computed != expected {
        return Err(IonError::from(format!("{name}: crc mismatch")));
    }
    Ok(())
}

mod arrays;
mod windows;
mod spectra;
mod to_mzml;

pub use arrays::{ArrayRef, ArrayGroup};
pub use windows::{ArrayWindow, MzPeaks, Target, ItemSlice};
pub use to_mzml::{Metadatum, MetadatumValue};

pub(crate) use arrays::{
    aref_read_params, array_ref_start_for_item, decode_from_block, dtype_stride, group_arrays,
    parse_array_pair, read_array_refs_from_buffers, read_group_decoded_bytes, unfilter_array_bytes,
};
pub(crate) use to_mzml::attach_logical_array;

#[derive(Debug, Clone)]
pub struct ReadOptions {
    pub max_cached_bytes: usize,
    pub verify_checksums: bool,
    pub parallel: bool,
    pub decompression_limit: DecompressionLimit,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            max_cached_bytes: DEFAULT_MAX_CACHED_BYTES,
            verify_checksums: true,
            parallel: true,
            decompression_limit: DecompressionLimit::default(),
        }
    }
}

pub(crate) enum SegmentBoundsCache {
    Unloaded,
    Missing,
    BadChecksum,
    Malformed(String),
    Loaded(SegmentBoundsIndex),
}

pub struct IonReader {
    pub(crate) header: Header,
    pub(crate) source: Arc<dyn ReadBytes>,
    pub(crate) spec_segment_bounds: SegmentBoundsCache,
    pub(crate) chrom_segment_bounds: SegmentBoundsCache,
    pub(crate) spec_summary_buf: Arc<[u8]>,
    pub(crate) chrom_summary_buf: Arc<[u8]>,
    pub(crate) spec_entries_buf: Arc<[u8]>,
    pub(crate) spec_array_refs: Arc<[u8]>,
    pub(crate) chrom_entries_buf: Arc<[u8]>,
    pub(crate) chrom_array_refs: Arc<[u8]>,
    pub(crate) global_meta_buf: Arc<[u8]>,
    pub(crate) spec_container: BlockReader<DefaultBlockProcessor>,
    pub(crate) chrom_container: Option<BlockReader<DefaultBlockProcessor>>,
    pub(crate) spec_meta_reader: MetaGroupReader,
    pub(crate) chrom_meta_reader: MetaGroupReader,
    pub(crate) mz_values: Vec<f64>,
    pub(crate) int_values: Vec<f64>,
    pub(crate) parallel: bool,
    pub(crate) decompression_limit: DecompressionLimit,
}

impl IonReader {
    pub fn open(bytes: &[u8], config: ReadOptions) -> IonResult<Self> {
        Self::open_bytes(Arc::from(bytes), config)
    }

    pub fn open_bytes(file_bytes: Arc<[u8]>, config: ReadOptions) -> IonResult<Self> {
        let source = Arc::new(BytesSource::new(file_bytes)) as Arc<dyn ReadBytes>;
        Self::open_source(source, config)
    }

    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    pub fn open_file(path: &std::path::Path, config: ReadOptions) -> IonResult<Self> {
        let file = std::fs::File::open(path).map_err(|e| IonError::from(e.to_string()))?;
        let map = unsafe { memmap2::Mmap::map(&file).map_err(|e| IonError::from(e.to_string()))? };
        let source = Arc::new(FileSource::new(map)) as Arc<dyn ReadBytes>;
        Self::open_source(source, config)
    }

    pub fn open_remote(
        read: impl Fn(Range) -> IonResult<Vec<u8>> + Send + Sync + 'static,
        config: ReadOptions,
    ) -> IonResult<Self> {
        let source = Arc::new(CallbackSource::new(read)) as Arc<dyn ReadBytes>;
        Self::open_source(source, config)
    }

    pub fn open_source(source: Arc<dyn ReadBytes>, config: ReadOptions) -> IonResult<Self> {
        let header_buf = source.read(Range { offset: 0, length: 1024 })?;
        let header = parse_header(&header_buf)?;
        let block_packing_id = PackingId::from_byte(header.default_array_filter)?;

        let spec_summary_buf = source.read(Range { offset: header.off_spec_summary, length: header.len_spec_summary })?;
        let chrom_summary_buf = source.read(Range { offset: header.off_chrom_summary, length: header.len_chrom_summary })?;
        let spec_entries_buf = source.read(Range { offset: header.off_spec_entries, length: header.len_spec_entries })?;
        let spec_array_refs = source.read(Range { offset: header.off_spec_arrayrefs, length: header.len_spec_arrayrefs })?;
        let chrom_entries_buf = source.read(Range { offset: header.off_chrom_entries, length: header.len_chrom_entries })?;
        let chrom_array_refs =
            source.read(Range { offset: header.off_chrom_arrayrefs, length: header.len_chrom_arrayrefs })?;
        let global_meta_buf = source.read(Range { offset: header.off_global_meta, length: header.len_global_meta })?;

        if config.verify_checksums {
            check_crc(&spec_summary_buf, header.spec_summary_crc32, "spec_summary")?;
            check_crc(&spec_entries_buf, header.spec_entries_crc32, "spec_entries")?;
            check_crc(&spec_array_refs, header.spec_arrayrefs_crc32, "spec_arrayrefs")?;
            check_crc(&chrom_summary_buf, header.chrom_summary_crc32, "chrom_summary")?;
            check_crc(&chrom_entries_buf, header.chrom_entries_crc32, "chrom_entries")?;
            check_crc(&chrom_array_refs, header.chrom_arrayrefs_crc32, "chrom_arrayrefs")?;
        }

        let spec_container = BlockReader::new(
            source.clone(),
            header.off_spec_container,
            header.len_spec_container,
            header.spec_block_count,
            header.spec_directory_crc32,
            header.compression_codec,
            block_packing_id,
            config.verify_checksums,
            "spec",
            DefaultBlockProcessor,
            config.max_cached_bytes,
            config.decompression_limit,
        )?;

        let chrom_container = if header.chrom_block_count > 0 && header.len_chrom_container > 0 {
            Some(BlockReader::new(
                source.clone(),
                header.off_chrom_container,
                header.len_chrom_container,
                header.chrom_block_count,
                header.chrom_directory_crc32,
                header.compression_codec,
                block_packing_id,
                config.verify_checksums,
                "chrom",
                DefaultBlockProcessor,
                config.max_cached_bytes,
                config.decompression_limit,
            )?)
        } else {
            None
        };

        let spec_meta_reader = MetaGroupReader::new(
            Arc::from(source.read(Range { offset: header.off_spec_meta, length: header.len_spec_meta })?),
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
            config.decompression_limit,
            config.max_cached_bytes,
        )?;

        let chrom_meta_reader = MetaGroupReader::new(
            Arc::from(source.read(Range { offset: header.off_chrom_meta, length: header.len_chrom_meta })?),
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
            config.decompression_limit,
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
            decompression_limit: config.decompression_limit,
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

    pub(crate) fn ensure_spec_segment_bounds(&mut self) -> IonResult<()> {
        if matches!(self.spec_segment_bounds, SegmentBoundsCache::Unloaded) {
            self.spec_segment_bounds = match self.load_spec_segment_bounds() {
                Ok(index) => SegmentBoundsCache::Loaded(index),
                Err(IonError::MissingSpectrumBounds) => SegmentBoundsCache::Missing,
                Err(IonError::BadSpectrumBoundsChecksum) => SegmentBoundsCache::BadChecksum,
                Err(IonError::MalformedSpectrumBounds(reason)) => {
                    SegmentBoundsCache::Malformed(reason)
                }
                Err(other) => SegmentBoundsCache::Malformed(other.to_string()),
            };
        }
        match &self.spec_segment_bounds {
            SegmentBoundsCache::Loaded(_) => Ok(()),
            SegmentBoundsCache::Missing => Err(IonError::MissingSpectrumBounds),
            SegmentBoundsCache::BadChecksum => Err(IonError::BadSpectrumBoundsChecksum),
            SegmentBoundsCache::Malformed(reason) => {
                Err(IonError::MalformedSpectrumBounds(reason.clone()))
            }
            SegmentBoundsCache::Unloaded => Err(IonError::MissingSpectrumBounds),
        }
    }

    pub(crate) fn ensure_chrom_segment_bounds(&mut self) {
        if !matches!(self.chrom_segment_bounds, SegmentBoundsCache::Unloaded) {
            return;
        }
        self.chrom_segment_bounds = match self.load_chrom_segment_bounds() {
            Some(index) => SegmentBoundsCache::Loaded(index),
            None => SegmentBoundsCache::Missing,
        };
    }

    fn load_spec_segment_bounds(&self) -> IonResult<SegmentBoundsIndex> {
        if self.header.len_spec_segment_bounds == 0 {
            return Err(IonError::MissingSpectrumBounds);
        }
        let spec_array_ref_count = self.spec_array_refs.len() as u64 / ARRAY_REF_BYTES as u64;
        let bytes = self
            .source
            .read(Range {
                offset: self.header.off_spec_segment_bounds,
                length: self.header.len_spec_segment_bounds,
            })
            .map_err(|error| IonError::MalformedSpectrumBounds(format!("read failed: {error}")))?;
        if crc32fast::hash(&bytes) != self.header.spec_segment_bounds_crc32 {
            return Err(IonError::BadSpectrumBoundsChecksum);
        }
        let decompressed = self
            .decompress_segment_bounds(&bytes, self.header.plain_len_spec_segment_bounds as usize)
            .map_err(|error| {
                IonError::MalformedSpectrumBounds(format!("decompression failed: {error}"))
            })?;
        SegmentBoundsIndex::from_bytes(&decompressed, spec_array_ref_count)
            .map_err(|error| IonError::MalformedSpectrumBounds(error.to_string()))
    }

    fn load_chrom_segment_bounds(&self) -> Option<SegmentBoundsIndex> {
        if self.header.len_chrom_segment_bounds == 0 {
            return None;
        }
        let chrom_array_ref_count = self.chrom_array_refs.len() as u64 / ARRAY_REF_BYTES as u64;
        let bytes = self
            .source
            .read(Range {
                offset: self.header.off_chrom_segment_bounds,
                length: self.header.len_chrom_segment_bounds,
            })
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
            CODEC_ZSTD => decompress_zstd(bytes, plain_len, self.decompression_limit),
            _ => Err("segment bounds: unsupported codec".into()),
        }
    }
}

pub fn plan_open_ranges(header_bytes: &[u8]) -> IonResult<Vec<Range>> {
    if header_bytes.len() < 1024 {
        return Err("plan_open_ranges: needs at least 1024 header bytes".into());
    }
    let header = parse_header(&header_bytes[..1024])?;
    open_byte_ranges(&header)
}

pub(crate) fn open_byte_ranges(header: &Header) -> IonResult<Vec<Range>> {
    let mut ranges = vec![
        Range { offset: 0, length: 1024 },
        Range { offset: header.off_spec_summary, length: header.len_spec_summary },
        Range { offset: header.off_chrom_summary, length: header.len_chrom_summary },
        Range { offset: header.off_spec_entries, length: header.len_spec_entries },
        Range { offset: header.off_spec_arrayrefs, length: header.len_spec_arrayrefs },
        Range { offset: header.off_chrom_entries, length: header.len_chrom_entries },
        Range { offset: header.off_chrom_arrayrefs, length: header.len_chrom_arrayrefs },
        Range { offset: header.off_global_meta, length: header.len_global_meta },
        Range { offset: header.off_spec_meta, length: header.len_spec_meta },
        Range { offset: header.off_chrom_meta, length: header.len_chrom_meta },
    ];

    if header.len_spec_segment_bounds > 0 {
        ranges.push(Range {
            offset: header.off_spec_segment_bounds,
            length: header.len_spec_segment_bounds,
        });
    }
    if header.len_chrom_segment_bounds > 0 {
        ranges.push(Range {
            offset: header.off_chrom_segment_bounds,
            length: header.len_chrom_segment_bounds,
        });
    }

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

#[cfg(test)]
mod tests;
