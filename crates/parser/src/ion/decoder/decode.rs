#![deny(clippy::undocumented_unsafe_blocks)]

use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use memmap2::Mmap;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use rayon::prelude::*;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use std::{fs::File, path::Path};

use crate::{
    accessions::format_accession,
    encoder::encode::{CHROM_SUMMARY_SIZE, SPEC_SUMMARY_SIZE},
    ion::{
        IonError, IonResult,
        attr_meta::{
            ACC_ATTR_DEFAULT_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_DEFAULT_SOURCE_FILE_REF,
            ACC_ATTR_ID, ACC_ATTR_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_REF, ACC_ATTR_SAMPLE_REF,
            ACC_ATTR_START_TIME_STAMP, parse_accession_tail,
        },
        encoder::encode::{
            FILE_DTYPE_F16, FILE_DTYPE_F32, FILE_DTYPE_F64, FILE_DTYPE_I16, FILE_DTYPE_I32,
            FILE_DTYPE_I64,
        },
        filter_summary::{ChromatogramSummary, SpectrumSummary},
        meta_groups::MetaTotals,
        packing::PackingId,
        utilities::{
            MetaGroupReader,
            children_lookup::{ChildrenLookup, DefaultMetadataPolicy, OwnerRows},
            common::get_attr_text,
            container_view::{ContainerAccess, ContainerView, DefaultProcessor},
            decompression_budget::DecompressionBudget,
            parse_chromatogram_list, parse_cv_and_user_params, parse_cv_list,
            parse_data_processing_list, parse_file_description,
            parse_global_metadata::parse_global_metadata,
            parse_header::{Header, parse_header},
            parse_instrument_list, parse_referenceable_param_group_list, parse_sample_list,
            parse_scan_settings_list, parse_software_list, parse_spectrum, parse_spectrum_list,
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

#[derive(Debug, Clone, Copy, Default)]
pub struct ArrayRef {
    pub block_id: u32,
    pub element_offset: u64,
    pub element_count: u64,
    pub array_type: u32,
    pub dtype: u8,
    pub array_filter: u8,
    pub encoded_len: u32,
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

pub struct Decoder<'a> {
    bytes: &'a [u8],
    header: Header,
    spec_container: ContainerView<'a, DefaultProcessor>,
    chrom_container: Option<ContainerView<'a, DefaultProcessor>>,
    spec_meta_reader: MetaGroupReader<'a>,
    chrom_meta_reader: MetaGroupReader<'a>,
    mz_buf: Vec<f64>,
    int_buf: Vec<f64>,
    parallel: bool,
    decompression_budget: DecompressionBudget,
}

#[allow(dead_code)]
enum IonBacking {
    Bytes(Arc<[u8]>),
    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    Map(Mmap),
}

pub struct OwnedIon {
    ion: Ion<'static>,
    _backing: IonBacking,
}

impl OwnedIon {
    pub fn open_bytes(data: Arc<[u8]>, config: DecoderConfig) -> IonResult<Self> {
        let raw = data.as_ref();
        // SAFETY: 'static is a lie. _backing owns the bytes; drops after ion.
        let bytes: &'static [u8] = unsafe { std::slice::from_raw_parts(raw.as_ptr(), raw.len()) };
        let ion = Ion::open(bytes, config)?;
        Ok(Self {
            ion,
            _backing: IonBacking::Bytes(data),
        })
    }

    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    pub fn open(path: &Path, config: DecoderConfig) -> IonResult<Self> {
        let file = File::open(path).map_err(|err| IonError::from(err.to_string()))?;
        // SAFETY: don't touch the file (modify/truncate) while OwnedIon is alive.
        let map = unsafe { Mmap::map(&file) }.map_err(|err| IonError::from(err.to_string()))?;
        let raw = map.as_ref();
        // SAFETY: same as open_bytes, _backing holds the Mmap.
        let bytes: &'static [u8] = unsafe { std::slice::from_raw_parts(raw.as_ptr(), raw.len()) };
        let ion = Ion::open(bytes, config)?;
        Ok(Self {
            ion,
            _backing: IonBacking::Map(map),
        })
    }

    #[inline]
    pub fn format_version(&self) -> Option<u16> {
        self.ion.format_version()
    }
}

impl Deref for OwnedIon {
    type Target = Ion<'static>;

    fn deref(&self) -> &Self::Target {
        &self.ion
    }
}

impl DerefMut for OwnedIon {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ion
    }
}

impl<'a> Decoder<'a> {
    pub fn open(bytes: &'a [u8], config: DecoderConfig) -> IonResult<Self> {
        let header = parse_header(bytes)?;
        let block_packing_id = PackingId::from_byte(header.default_array_filter)?;

        let spec_container = {
            let off = usize::try_from(header.off_spec_container)
                .map_err(|_| IonError::from("spectrum container out of bounds"))?;
            let len = usize::try_from(header.len_spec_container)
                .map_err(|_| IonError::from("spectrum container out of bounds"))?;
            let end = off
                .checked_add(len)
                .ok_or_else(|| IonError::from("spectrum container out of bounds"))?;
            let cb = bytes
                .get(off..end)
                .ok_or_else(|| IonError::from("spectrum container out of bounds"))?;
            ContainerView::with_max_cached_bytes(
                cb,
                header.spec_block_count,
                header.compression_level,
                block_packing_id,
                config.verify_checksums,
                "spec",
                DefaultProcessor,
                config.max_cached_bytes,
                config.decompression_budget,
            )?
        };

        let chrom_container = if header.chrom_block_count > 0 && header.len_chrom_container > 0 {
            let off = usize::try_from(header.off_chrom_container)
                .map_err(|_| IonError::from("chrom container out of bounds"))?;
            let len = usize::try_from(header.len_chrom_container)
                .map_err(|_| IonError::from("chrom container out of bounds"))?;
            let end = off
                .checked_add(len)
                .ok_or_else(|| IonError::from("chrom container out of bounds"))?;
            let container_bytes = bytes
                .get(off..end)
                .ok_or_else(|| IonError::from("chrom container out of bounds"))?;
            Some(ContainerView::with_max_cached_bytes(
                container_bytes,
                header.chrom_block_count,
                header.compression_level,
                block_packing_id,
                config.verify_checksums,
                "chrom",
                DefaultProcessor,
                config.max_cached_bytes,
                config.decompression_budget,
            )?)
        } else {
            None
        };

        let spec_meta_reader = MetaGroupReader::new(
            slice_at(
                bytes,
                header.off_spec_meta,
                header.len_spec_meta,
                "spec_meta",
            )?,
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
            slice_at(
                bytes,
                header.off_chrom_meta,
                header.len_chrom_meta,
                "chrom_meta",
            )?,
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
            bytes,
            header,
            spec_container,
            chrom_container,
            spec_meta_reader,
            chrom_meta_reader,
            mz_buf: Vec::new(),
            int_buf: Vec::new(),
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
            &self.bytes,
            self.header.off_spec_summary,
            index,
            SPEC_SUMMARY_SIZE,
            self.header.spectrum_count,
        )?;
        Some(parse_spec_summary(b))
    }

    pub fn spec_summaries(&self) -> IonResult<Vec<SpectrumSummary>> {
        let off = usize::try_from(self.header.off_spec_summary)
            .map_err(|_| IonError::from("spec summary: out of bounds"))?;
        let len = usize::try_from(self.header.len_spec_summary)
            .map_err(|_| IonError::from("spec summary: out of bounds"))?;
        let count = usize::try_from(self.header.spectrum_count)
            .map_err(|_| IonError::from("spec summary: out of bounds"))?;
        if len != count * SPEC_SUMMARY_SIZE {
            return Err(
                format!("spec summary: len={len} != count={count} × {SPEC_SUMMARY_SIZE}").into(),
            );
        }
        let end = off
            .checked_add(len)
            .ok_or_else(|| IonError::from("spec summary: out of bounds"))?;
        let section = self
            .bytes
            .get(off..end)
            .ok_or_else(|| IonError::from("spec summary: out of bounds"))?;
        Ok(section
            .chunks_exact(SPEC_SUMMARY_SIZE)
            .map(parse_spec_summary)
            .collect())
    }

    pub fn chrom_summary(&self, index: usize) -> Option<ChromatogramSummary> {
        let b = slice_summary(
            &self.bytes,
            self.header.off_chrom_summary,
            index,
            CHROM_SUMMARY_SIZE,
            self.header.chrom_count,
        )?;
        Some(parse_chrom_summary(b))
    }

    pub fn chrom_summaries(&self) -> IonResult<Vec<ChromatogramSummary>> {
        let off = usize::try_from(self.header.off_chrom_summary)
            .map_err(|_| IonError::from("chrom summary: out of bounds"))?;
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
        let end = off
            .checked_add(len)
            .ok_or_else(|| IonError::from("chrom summary: out of bounds"))?;
        let section = self
            .bytes
            .get(off..end)
            .ok_or_else(|| IonError::from("chrom summary: out of bounds"))?;
        Ok(section
            .chunks_exact(CHROM_SUMMARY_SIZE)
            .map(parse_chrom_summary)
            .collect())
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
        .map(ArrayRefs::into_vec)
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
        .map(ArrayRefs::into_vec)
    }

    pub fn read_spectrum_array(&mut self, aref: &ArrayRef, out: &mut Vec<f64>) -> IonResult<()> {
        let (element_offset, count, stride) = aref_read_params(aref);
        let raw = self.spec_container.get_item_from_block(
            aref.block_id,
            element_offset,
            count,
            stride,
            "read_spectrum_array",
        )?;
        decode_into(out, raw, aref.dtype, aref.array_filter)
    }

    pub fn read_chromatogram_array(
        &mut self,
        aref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        let container = self
            .chrom_container
            .as_mut()
            .ok_or_else(|| IonError::from("no chromatogram container"))?;
        let (element_offset, count, stride) = aref_read_params(aref);
        let raw = container.get_item_from_block(
            aref.block_id,
            element_offset,
            count,
            stride,
            "read_chromatogram_array",
        )?;
        decode_into(out, raw, aref.dtype, aref.array_filter)
    }

    pub(crate) fn global_metadata(&self) -> IonResult<Vec<Metadatum>> {
        parse_global_metadata(
            slice_at(
                self.bytes,
                self.header.off_global_meta,
                self.header.len_global_meta,
                "global",
            )?,
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

        if let Some(arefs) = read_array_refs_at(
            self.bytes,
            self.header.off_spec_entries as usize,
            self.header.off_spec_arrayrefs as usize,
            index,
        ) {
            let bd_list = spectrum
                .binary_data_array_list
                .get_or_insert_with(BinaryDataArrayList::default);
            for aref in arefs.as_slice() {
                let (eo, count, stride) = aref_read_params(aref);
                let raw = self.spec_container.get_item_from_block(
                    aref.block_id,
                    eo,
                    count,
                    stride,
                    "spectrum_at",
                )?;
                attach_array(bd_list, aref.array_type, aref.dtype, raw, aref.array_filter)?;
            }
            bd_list.count = Some(bd_list.binary_data_arrays.len());
        }
        Ok(Some(spectrum))
    }
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
enum IonBackend<'a> {
    Decoder(Decoder<'a>),
    Data,
}

impl<'a> IonBackend<'a> {
    fn as_decoder(&self) -> Option<&Decoder<'a>> {
        match self {
            Self::Decoder(d) => Some(d),
            Self::Data => None,
        }
    }

    fn as_decoder_mut(&mut self) -> Option<&mut Decoder<'a>> {
        match self {
            Self::Decoder(d) => Some(d),
            Self::Data => None,
        }
    }
}

pub struct Ion<'a> {
    pub cv_list: Option<CvList>,
    pub file_description: Option<FileDescription>,
    pub referenceable_param_group_list: Option<ReferenceableParamGroupList>,
    pub sample_list: Option<SampleList>,
    pub instrument_list: Option<InstrumentList>,
    pub software_list: Option<SoftwareList>,
    pub data_processing_list: Option<DataProcessingList>,
    pub scan_settings_list: Option<ScanSettingsList>,
    pub run: Run,
    backend: IonBackend<'a>,
}

impl<'a> Ion<'a> {
    pub fn open(bytes: &'a [u8], config: DecoderConfig) -> IonResult<Self> {
        let decoder = Decoder::open(bytes, config)?;
        Ok(Self::empty(IonBackend::Decoder(decoder)))
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
    fn empty(backend: IonBackend<'a>) -> Self {
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
            IonBackend::Data => None,
        };
        if let Some(mzml) = mzml {
            self.set_from_mzml(mzml);
        }
        Ok(())
    }

    #[inline]
    pub fn spectrum_count(&self) -> u64 {
        self.backend
            .as_decoder()
            .map(|d| d.spectrum_count())
            .unwrap_or_else(|| {
                self.run
                    .spectrum_list
                    .as_ref()
                    .map_or(0, |l| l.spectra.len() as u64)
            })
    }

    #[inline]
    pub fn chromatogram_count(&self) -> u64 {
        self.backend
            .as_decoder()
            .map(|d| d.chromatogram_count())
            .unwrap_or_else(|| {
                self.run
                    .chromatogram_list
                    .as_ref()
                    .map_or(0, |l| l.chromatograms.len() as u64)
            })
    }

    #[inline]
    pub fn format_version(&self) -> Option<u16> {
        self.backend.as_decoder().map(|d| d.format_version())
    }

    #[inline]
    pub fn spec_summary(&self, index: usize) -> Option<SpectrumSummary> {
        self.backend
            .as_decoder()
            .and_then(|d| d.spec_summary(index))
    }

    pub fn spec_summaries(&self) -> IonResult<Vec<SpectrumSummary>> {
        self.backend
            .as_decoder()
            .ok_or_else(|| {
                IonError::from("spec summary summaries are unavailable for mzML-backed Ion")
            })
            .and_then(|d| d.spec_summaries())
    }

    #[inline]
    pub fn chrom_summary(&self, index: usize) -> Option<ChromatogramSummary> {
        self.backend
            .as_decoder()
            .and_then(|d| d.chrom_summary(index))
    }

    pub fn chrom_summaries(&self) -> IonResult<Vec<ChromatogramSummary>> {
        self.backend
            .as_decoder()
            .ok_or_else(|| {
                IonError::from("chrom summary summaries are unavailable for mzML-backed Ion")
            })
            .and_then(|d| d.chrom_summaries())
    }

    pub fn spectrum_array_refs(&self, index: usize) -> Option<Vec<ArrayRef>> {
        self.backend
            .as_decoder()
            .and_then(|d| d.spectrum_array_refs(index))
    }

    pub fn chromatogram_array_refs(&self, index: usize) -> Option<Vec<ArrayRef>> {
        self.backend
            .as_decoder()
            .and_then(|d| d.chromatogram_array_refs(index))
    }

    pub fn read_spectrum_array(&mut self, aref: &ArrayRef, out: &mut Vec<f64>) -> IonResult<()> {
        self.backend
            .as_decoder_mut()
            .ok_or_else(|| IonError::from("array refs are unavailable for mzML-backed Ion"))
            .and_then(|d| d.read_spectrum_array(aref, out))
    }

    pub fn read_chromatogram_array(
        &mut self,
        aref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        self.backend
            .as_decoder_mut()
            .ok_or_else(|| IonError::from("array refs are unavailable for mzML-backed Ion"))
            .and_then(|d| d.read_chromatogram_array(aref, out))
    }

    pub fn to_mzml(&mut self) -> IonResult<MzML> {
        self.backend
            .as_decoder_mut()
            .map(|d| d.to_mzml())
            .unwrap_or_else(|| Ok(self.clone_as_mzml()))
    }

    pub fn to_mzml_metadata_only(&self) -> IonResult<MzML> {
        self.backend
            .as_decoder()
            .map(|d| d.to_mzml_metadata_only())
            .unwrap_or_else(|| Ok(self.clone_as_mzml_metadata_only()))
    }

    pub fn spectrum_at(&mut self, index: usize) -> IonResult<Option<Spectrum>> {
        match &mut self.backend {
            IonBackend::Decoder(d) => d.spectrum_at(index),
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
            IonBackend::Data => Err(IonError::from(
                "metadata rows are only available on a file-backed Ion (use Ion::open)",
            )),
        }
    }

    pub fn chromatogram_metadata_at(&mut self, index: usize) -> IonResult<Vec<Metadatum>> {
        match &mut self.backend {
            IonBackend::Decoder(d) => d.chromatogram_metadata_at(index),
            IonBackend::Data => Err(IonError::from(
                "metadata rows are only available on a file-backed Ion (use Ion::open)",
            )),
        }
    }
}

impl ScanSource for Decoder<'_> {
    fn for_each_summary(&mut self, callback: &mut dyn FnMut(usize, ScanSummary)) {
        let Some(summary_bytes) =
            usize::try_from(self.header.off_spec_summary)
                .ok()
                .and_then(|off| {
                    usize::try_from(self.header.len_spec_summary)
                        .ok()
                        .and_then(|len| {
                            off.checked_add(len)
                                .and_then(|end| self.bytes.get(off..end))
                        })
                })
        else {
            return;
        };
        for (index, chunk) in summary_bytes.chunks_exact(SPEC_SUMMARY_SIZE).enumerate() {
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
        let Some(all_entries) = count.checked_mul(INDEX_ENTRY_BYTES).and_then(|total| {
            usize::try_from(self.header.off_spec_entries)
                .ok()
                .and_then(|off| {
                    off.checked_add(total)
                        .and_then(|end| self.bytes.get(off..end))
                })
        }) else {
            return false;
        };
        let array_ref_bytes = match usize::try_from(self.header.off_spec_arrayrefs) {
            Ok(off) => self.bytes.get(off..).unwrap_or(&[]),
            Err(_) => return false,
        };
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
        let Some(summary_bytes) =
            self.header
                .len_spec_summary
                .try_into()
                .ok()
                .and_then(|len: usize| {
                    usize::try_from(self.header.off_spec_summary)
                        .ok()
                        .and_then(|off| {
                            off.checked_add(len)
                                .and_then(|end| self.bytes.get(off..end))
                        })
                })
        else {
            return;
        };
        let count = match usize::try_from(self.header.spectrum_count) {
            Ok(count) => count,
            Err(_) => return,
        };
        let Some(entry_bytes) = count.checked_mul(INDEX_ENTRY_BYTES).and_then(|len| {
            usize::try_from(self.header.off_spec_entries)
                .ok()
                .and_then(|off| {
                    off.checked_add(len)
                        .and_then(|end| self.bytes.get(off..end))
                })
        }) else {
            return;
        };
        let array_ref_bytes = match usize::try_from(self.header.off_spec_arrayrefs) {
            Ok(off) => self.bytes.get(off..).unwrap_or(&[]),
            Err(_) => return,
        };
        let (container, mz_buf, int_buf) = (
            &mut self.spec_container as &mut dyn ContainerAccess,
            &mut self.mz_buf,
            &mut self.int_buf,
        );
        ScanIterator::new(
            summary_bytes,
            entry_bytes,
            array_ref_bytes,
            container,
            mz_buf,
            int_buf,
            rt_min * 60.0,
            rt_max * 60.0,
            ms_level,
        )
        .run(&mut callback);
    }
}

impl<'a> ScanSource for Ion<'a> {
    fn for_each_summary(&mut self, callback: &mut dyn FnMut(usize, ScanSummary)) {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => decoder.for_each_summary(callback),
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

struct MzmlConverter<'a, 'd> {
    decoder: &'d mut Decoder<'a>,
}

impl<'a, 'd> MzmlConverter<'a, 'd> {
    #[inline]
    fn new(decoder: &'d mut Decoder<'a>) -> Self {
        Self { decoder }
    }

    fn metadata_only(decoder: &Decoder<'a>) -> IonResult<MzML> {
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
                self.decoder.bytes,
                self.decoder.header.off_spec_entries as usize,
                self.decoder.header.off_spec_arrayrefs as usize,
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
                self.decoder.bytes,
                self.decoder.header.off_chrom_entries as usize,
                self.decoder.header.off_chrom_arrayrefs as usize,
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
    mz_buf: &'d mut Vec<f64>,
    int_buf: &'d mut Vec<f64>,
    rt_min_s: f64,
    rt_max_s: f64,
    ms_level: u8,
}

impl<'a, 'd> ScanIterator<'a, 'd> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        summary_bytes: &'a [u8],
        entry_bytes: &'a [u8],
        aref_bytes: &'a [u8],
        container: &'d mut dyn ContainerAccess,
        mz_buf: &'d mut Vec<f64>,
        int_buf: &'d mut Vec<f64>,
        rt_min_s: f64,
        rt_max_s: f64,
        ms_level: u8,
    ) -> Self {
        Self {
            summary_chunks: summary_bytes.chunks_exact(SPEC_SUMMARY_SIZE),
            entry_chunks: entry_bytes.chunks_exact(INDEX_ENTRY_BYTES),
            aref_bytes,
            container,
            mz_buf,
            int_buf,
            rt_min_s,
            rt_max_s,
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
                || summary.rt_seconds < self.rt_min_s
                || summary.rt_seconds > self.rt_max_s
            {
                continue;
            }
            if self.ms_level != 0 && summary.ms_level != self.ms_level {
                continue;
            }
            let Some((mz_ref, int_ref)) = parse_array_pair(entry_bytes, self.aref_bytes) else {
                continue;
            };
            if !decode_from_block(self.container, self.mz_buf, &mz_ref) {
                continue;
            }
            if !decode_from_block(self.container, self.int_buf, &int_ref) {
                continue;
            }
            let len = self.mz_buf.len().min(self.int_buf.len());
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
            callback(&summary, &self.mz_buf[..len], &self.int_buf[..len]);
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
struct ArrayRefs {
    len: usize,
    inline: [ArrayRef; INLINE_ARRAY_REF_CAP],
    heap: Option<Vec<ArrayRef>>,
}

impl ArrayRefs {
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
fn read_array_refs_at(
    bytes: &[u8],
    entry_base: usize,
    aref_base: usize,
    index: usize,
) -> Option<ArrayRefs> {
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
    let mut refs = ArrayRefs::with_capacity(ref_count);
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
    }
}

#[inline]
fn aref_read_params(aref: &ArrayRef) -> (u64, u64, usize) {
    if aref.encoded_len > 0 {
        (aref.element_offset, aref.encoded_len as u64, 1)
    } else {
        (
            aref.element_offset,
            aref.element_count,
            dtype_stride(aref.dtype),
        )
    }
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
    aref: &ArrayRef,
) -> bool {
    let (element_offset, count, stride) = aref_read_params(aref);
    match container.get_item_from_block(aref.block_id, element_offset, count, stride, "scan") {
        Ok(raw) => decode_into(buf, raw, aref.dtype, aref.array_filter).is_ok(),
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
    bytes: &[u8],
    entry_base: usize,
    aref_base: usize,
    entries: &mut [E],
    container: &ContainerView<'_, DefaultProcessor>,
    ctx: &'static str,
    parallel: bool,
) -> IonResult<()> {
    let mut refs = Vec::new();
    let mut blocks = HashMap::new();
    #[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
    let _ = parallel;

    for index in 0..entries.len() {
        let Some(item_refs) = read_array_refs_at(bytes, entry_base, aref_base, index) else {
            continue;
        };
        if item_refs.is_empty() {
            continue;
        }
        for aref in item_refs.as_slice() {
            let stride = if aref.encoded_len > 0 {
                1
            } else {
                dtype_stride(aref.dtype)
            };
            if let Some(old) = blocks.insert(aref.block_id, stride)
                && old != stride
            {
                return Err(IonError::from(format!(
                    "{ctx}: stride mismatch for block {} (expected {old}, got {stride})",
                    aref.block_id
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
        let list = entries[index]
            .binary_data_array_list_mut()
            .get_or_insert_with(BinaryDataArrayList::default);
        for aref in item_refs.as_slice() {
            let block = data
                .get(&aref.block_id)
                .ok_or_else(|| IonError::from(format!("{ctx}: missing block {}", aref.block_id)))?;
            let (start, end) = {
                let (element_offset, count, stride) = aref_read_params(aref);
                let s = usize::try_from(element_offset)
                    .ok()
                    .and_then(|offset| offset.checked_mul(stride))
                    .ok_or_else(|| {
                        IonError::from(format!(
                            "{ctx}: item range overflow for block {}",
                            aref.block_id
                        ))
                    })?;
                let e = usize::try_from(count)
                    .ok()
                    .and_then(|c| c.checked_mul(stride))
                    .and_then(|len| s.checked_add(len))
                    .ok_or_else(|| {
                        IonError::from(format!(
                            "{ctx}: item range overflow for block {}",
                            aref.block_id
                        ))
                    })?;
                (s, e)
            };
            let raw = block.get(start..end).ok_or_else(|| {
                IonError::from(format!(
                    "{ctx}: item range [{start}..{end}] out of bounds for block {} (len={})",
                    aref.block_id,
                    block.len()
                ))
            })?;
            attach_array(list, aref.array_type, aref.dtype, raw, aref.array_filter)?;
        }
        list.count = Some(list.binary_data_arrays.len());
    }

    Ok(())
}

fn attach_array(
    binary_array_list: &mut BinaryDataArrayList,
    array_type: u32,
    dtype: u8,
    raw: &[u8],
    array_filter: u8,
) -> IonResult<()> {
    let binary = raw_to_binary_data(raw, dtype, array_filter)?;
    let numeric_type = dtype_to_numeric_type(dtype)?;
    let found = binary_array_list
        .binary_data_arrays
        .iter_mut()
        .find(|array| binary_array_has_type(array, array_type));
    let binary_array = match found {
        Some(existing) => existing,
        None => {
            binary_array_list
                .binary_data_arrays
                .push(make_binary_array_stub(array_type));
            binary_array_list.binary_data_arrays.last_mut().unwrap()
        }
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

fn raw_to_binary_data(raw: &[u8], dtype: u8, array_filter: u8) -> IonResult<BinaryData> {
    let bytes = unfilter_array_bytes(raw, dtype, array_filter)?;
    match dtype {
        FILE_DTYPE_F64 => Ok(BinaryData::F64(raw_to_vec(&bytes, 8, |c| {
            f64::from_le_bytes(c.try_into().unwrap())
        })?)),
        FILE_DTYPE_F32 => Ok(BinaryData::F32(raw_to_vec(&bytes, 4, |c| {
            f32::from_le_bytes(c.try_into().unwrap())
        })?)),
        FILE_DTYPE_F16 => Ok(BinaryData::F16(raw_to_vec(&bytes, 2, |c| {
            u16::from_le_bytes(c.try_into().unwrap())
        })?)),
        FILE_DTYPE_I16 => Ok(BinaryData::I16(raw_to_vec(&bytes, 2, |c| {
            i16::from_le_bytes(c.try_into().unwrap())
        })?)),
        FILE_DTYPE_I32 => Ok(BinaryData::I32(raw_to_vec(&bytes, 4, |c| {
            i32::from_le_bytes(c.try_into().unwrap())
        })?)),
        FILE_DTYPE_I64 => Ok(BinaryData::I64(raw_to_vec(&bytes, 8, |c| {
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
fn is_numeric_acc(tail: crate::ion::attr_meta::AccessionTail) -> bool {
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

    #[test]
    fn owned_ion_ion_field_declared_before_backing() {
        let src = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/ion/decoder/decode.rs"
        ));
        let struct_start = src
            .find("pub struct OwnedIon {")
            .expect("OwnedIon struct missing");
        let body_start = struct_start + "pub struct OwnedIon {".len();
        let body_len = src[body_start..]
            .find('}')
            .expect("OwnedIon struct unclosed");
        let body = &src[body_start..body_start + body_len];
        let ion_pos = body.find("ion:").expect("ion field missing");
        let backing_pos = body.find("_backing:").expect("_backing field missing");
        assert!(
            ion_pos < backing_pos,
            "ion must precede _backing in OwnedIon"
        );
    }

    const BYTES: &[u8] = include_bytes!("../../../data/ion/test.ion");

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
}
