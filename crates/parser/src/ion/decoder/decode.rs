use crate::encoder::encode::FILTER_INDEX_RECORD_SIZE;
use crate::ion::attr_meta::ACC_ATTR_DEFAULT_SOURCE_FILE_REF;
use crate::ion::encoder::utilities::container_builder::FilterType;
use crate::ion::utilities::spectrum_source::ScanMeta;
use crate::ion::utilities::{
    container_view::{ContainerAccess, ContainerView, DefaultProcessor},
    parse_header::{Header, parse_header},
    spectrum_source::{SpectrumSource, f16_bits_to_f64, for_each_spectra_in_range},
};
use crate::ion::{
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
};
use crate::mzml::structs::FilterRecord;
use crate::mzml::{schema::TagId, structs::*};

const ACC_MZ: u32 = 1_000_514;
const ACC_INT: u32 = 1_000_515;
const ENTRY_A_BYTES: usize = 16;
const ENTRY_A1_BYTES: usize = 32;
const DEFAULT_MAX_CACHED_BYTES: usize = 256 * 1024 * 1024;
const INLINE_ARRAY_REF_CAP: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MetadatumValue {
    Number(f64),
    Text(String),
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Metadatum {
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
    f: &str,
) -> Result<&'a [u8], String> {
    let end = off
        .checked_add(len)
        .ok_or_else(|| format!("{f}: range error"))?;
    let start = usize::try_from(off).map_err(|_| format!("{f}: range error"))?;
    let end = usize::try_from(end).map_err(|_| format!("{f}: range error"))?;
    bytes
        .get(start..end)
        .ok_or_else(|| format!("{f}: range error"))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ArrayRef {
    pub block_id: u32,
    pub element_offset: u64,
    pub element_count: u64,
    pub array_type: u32,
    pub dtype: u8,
}

#[derive(Debug, Clone)]
pub struct DecoderConfig {
    pub max_cached_bytes: usize,
}

impl Default for DecoderConfig {
    fn default() -> Self {
        Self {
            max_cached_bytes: DEFAULT_MAX_CACHED_BYTES,
        }
    }
}

pub struct Decoder<'a> {
    bytes: &'a [u8],
    header: Header,
    spec_container: ContainerView<'a, DefaultProcessor>,
    chrom_container: Option<ContainerView<'a, DefaultProcessor>>,
    mz_buf: Vec<f64>,
    int_buf: Vec<f64>,
}

impl<'a> Decoder<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Self, String> {
        Self::open_with_config(bytes, DecoderConfig::default())
    }

    pub fn open_with_config(bytes: &'a [u8], config: DecoderConfig) -> Result<Self, String> {
        let header = parse_header(bytes)?;
        let filter = FilterType::try_from(header.array_filter).unwrap_or(FilterType::None);

        let spec_container = {
            let off = usize::try_from(header.off_container_spect)
                .map_err(|_| "spectrum container out of bounds".to_string())?;
            let len = usize::try_from(header.len_container_spect)
                .map_err(|_| "spectrum container out of bounds".to_string())?;
            let end = off
                .checked_add(len)
                .ok_or_else(|| "spectrum container out of bounds".to_string())?;
            let cb = bytes
                .get(off..end)
                .ok_or_else(|| "spectrum container out of bounds".to_string())?;
            ContainerView::with_max_cached_bytes(
                cb,
                header.block_count_spect,
                header.compression_level,
                filter,
                "spec",
                DefaultProcessor,
                config.max_cached_bytes,
            )?
        };

        let chrom_container = if header.block_count_chrom > 0 && header.len_container_chrom > 0 {
            let off = usize::try_from(header.off_container_chrom)
                .map_err(|_| "chrom container out of bounds".to_string())?;
            let len = usize::try_from(header.len_container_chrom)
                .map_err(|_| "chrom container out of bounds".to_string())?;
            let end = off
                .checked_add(len)
                .ok_or_else(|| "chrom container out of bounds".to_string())?;
            let cb = bytes
                .get(off..end)
                .ok_or_else(|| "chrom container out of bounds".to_string())?;
            Some(ContainerView::with_max_cached_bytes(
                cb,
                header.block_count_chrom,
                header.compression_level,
                filter,
                "chrom",
                DefaultProcessor,
                config.max_cached_bytes,
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
        let base = self
            .header
            .off_filter_index
            .try_into()
            .ok()
            .and_then(|off: usize| {
                index
                    .checked_mul(FILTER_INDEX_RECORD_SIZE)
                    .and_then(|delta| off.checked_add(delta))
            })?;
        let end = base.checked_add(FILTER_INDEX_RECORD_SIZE)?;
        let b = self.bytes.get(base..end)?;
        Some(parse_filter_record(b))
    }

    pub fn filter_records(&self) -> Result<Vec<FilterRecord>, String> {
        let off = usize::try_from(self.header.off_filter_index)
            .map_err(|_| "filter index: out of bounds".to_string())?;
        let len = usize::try_from(self.header.len_filter_index)
            .map_err(|_| "filter index: out of bounds".to_string())?;
        let count = usize::try_from(self.header.spectrum_count)
            .map_err(|_| "filter index: out of bounds".to_string())?;
        if len != count * FILTER_INDEX_RECORD_SIZE {
            return Err(format!(
                "filter index: len={len} != count={count} × {FILTER_INDEX_RECORD_SIZE}"
            ));
        }
        let end = off
            .checked_add(len)
            .ok_or_else(|| "filter index: out of bounds".to_string())?;
        let section = self
            .bytes
            .get(off..end)
            .ok_or_else(|| "filter index: out of bounds".to_string())?;
        Ok(section
            .chunks_exact(FILTER_INDEX_RECORD_SIZE)
            .map(parse_filter_record)
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

    pub fn read_spectrum_array(
        &mut self,
        aref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> Result<(), String> {
        let raw = self.spec_container.get_item_from_block(
            aref.block_id,
            aref.element_offset,
            aref.element_count,
            dtype_stride(aref.dtype),
            "read_spectrum_array",
        )?;
        decode_into(out, raw, aref.dtype);
        Ok(())
    }

    pub fn read_chromatogram_array(
        &mut self,
        aref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> Result<(), String> {
        let container = self
            .chrom_container
            .as_mut()
            .ok_or_else(|| "no chromatogram container".to_string())?;
        let raw = container.get_item_from_block(
            aref.block_id,
            aref.element_offset,
            aref.element_count,
            dtype_stride(aref.dtype),
            "read_chromatogram_array",
        )?;
        decode_into(out, raw, aref.dtype);
        Ok(())
    }

    pub(crate) fn global_metadata(&self) -> Result<Vec<Metadatum>, String> {
        parse_global_metadata(
            slice_at(
                self.bytes,
                self.header.off_global_meta,
                self.header.len_global_meta,
                "global",
            )?,
            0,
            self.header.global_meta_count,
            self.header.global_meta_num_count,
            self.header.global_meta_str_count,
            self.header.compression_codec,
            self.header.global_meta_uncompressed_bytes,
        )
    }

    pub(crate) fn spectrum_metadata(&self) -> Result<Vec<Metadatum>, String> {
        parse_metadata(
            slice_at(
                self.bytes,
                self.header.off_spec_meta,
                self.header.len_spec_meta,
                "spec_meta",
            )?,
            self.header.spectrum_count,
            self.header.spec_meta_count,
            self.header.spec_meta_num_count,
            self.header.spec_meta_str_count,
            self.header.compression_codec,
            self.header.spec_meta_uncompressed_bytes as usize,
        )
    }

    pub(crate) fn chromatogram_metadata(&self) -> Result<Vec<Metadatum>, String> {
        parse_metadata(
            slice_at(
                self.bytes,
                self.header.off_chrom_meta,
                self.header.len_chrom_meta,
                "chrom_meta",
            )?,
            self.header.chrom_count,
            self.header.chrom_meta_count,
            self.header.chrom_meta_num_count,
            self.header.chrom_meta_str_count,
            self.header.compression_codec,
            self.header.chrom_meta_uncompressed_bytes as usize,
        )
    }

    pub fn to_mzml_metadata_only(&self) -> Result<MzML, String> {
        MzmlConverter::metadata_only(self)
    }

    pub fn to_mzml(&mut self) -> Result<MzML, String> {
        MzmlConverter::new(self).full()
    }
}

enum IonBackend<'a> {
    Decoder(Decoder<'a>),
    Materialized,
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
    pub fn open(bytes: &'a [u8]) -> Result<Self, String> {
        Self::open_with_config(bytes, DecoderConfig::default())
    }

    pub fn open_with_config(bytes: &'a [u8], config: DecoderConfig) -> Result<Self, String> {
        let decoder = Decoder::open_with_config(bytes, config)?;
        Ok(Self::empty(IonBackend::Decoder(decoder)))
    }

    pub fn from_mzml(mzml: MzML) -> Self {
        let mut ion = Self::empty(IonBackend::Materialized);
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

    pub fn load_metadata(&mut self) -> Result<(), String> {
        let mzml = match &mut self.backend {
            IonBackend::Decoder(decoder) => Some(decoder.to_mzml_metadata_only()?),
            IonBackend::Materialized => None,
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
            IonBackend::Materialized => self
                .run
                .spectrum_list
                .as_ref()
                .map_or(0, |list| list.spectra.len() as u64),
        }
    }

    #[inline]
    pub fn chromatogram_count(&self) -> u64 {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.chromatogram_count(),
            IonBackend::Materialized => self
                .run
                .chromatogram_list
                .as_ref()
                .map_or(0, |list| list.chromatograms.len() as u64),
        }
    }

    #[inline]
    pub fn header(&self) -> &Header {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.header(),
            IonBackend::Materialized => panic!("header is unavailable for mzML-backed Ion"),
        }
    }

    #[inline]
    pub fn filter_record(&self, index: usize) -> Option<FilterRecord> {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.filter_record(index),
            IonBackend::Materialized => None,
        }
    }

    pub fn filter_records(&self) -> Result<Vec<FilterRecord>, String> {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.filter_records(),
            IonBackend::Materialized => {
                Err("filter records are unavailable for mzML-backed Ion".to_string())
            }
        }
    }

    pub fn spectrum_array_refs(&self, index: usize) -> Option<Vec<ArrayRef>> {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.spectrum_array_refs(index),
            IonBackend::Materialized => None,
        }
    }

    pub fn chromatogram_array_refs(&self, index: usize) -> Option<Vec<ArrayRef>> {
        match &self.backend {
            IonBackend::Decoder(decoder) => decoder.chromatogram_array_refs(index),
            IonBackend::Materialized => None,
        }
    }

    pub fn read_spectrum_array(
        &mut self,
        aref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> Result<(), String> {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => decoder.read_spectrum_array(aref, out),
            IonBackend::Materialized => {
                Err("array refs are unavailable for mzML-backed Ion".to_string())
            }
        }
    }

    pub fn read_chromatogram_array(
        &mut self,
        aref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> Result<(), String> {
        match &mut self.backend {
            IonBackend::Decoder(decoder) => decoder.read_chromatogram_array(aref, out),
            IonBackend::Materialized => {
                Err("array refs are unavailable for mzML-backed Ion".to_string())
            }
        }
    }

    pub fn to_mzml(&mut self) -> Result<MzML, String> {
        if let IonBackend::Decoder(decoder) = &mut self.backend {
            return decoder.to_mzml();
        }
        Ok(self.clone_as_mzml())
    }

    pub fn to_mzml_metadata_only(&self) -> Result<MzML, String> {
        if let IonBackend::Decoder(decoder) = &self.backend {
            return decoder.to_mzml_metadata_only();
        }
        Ok(self.clone_as_mzml())
    }
}

impl<'a> SpectrumSource for Ion<'a> {
    fn for_each_scan_in_range(
        &mut self,
        rt_min: f64,
        rt_max: f64,
        ms_level: u8,
        callback: &mut dyn FnMut(f64, &ScanMeta, &[f64], &[f64]),
    ) {
        if let IonBackend::Decoder(decoder) = &mut self.backend {
            decoder.for_each_scan_in_range(rt_min, rt_max, ms_level, callback);
            return;
        }
        if let Some(list) = self.run.spectrum_list.as_ref() {
            for_each_spectra_in_range(&list.spectra, rt_min, rt_max, ms_level, callback);
        }
    }
}

impl<'a> SpectrumSource for Decoder<'a> {
    fn for_each_scan_in_range(
        &mut self,
        rt_min: f64,
        rt_max: f64,
        ms_level: u8,
        callback: &mut dyn FnMut(f64, &ScanMeta, &[f64], &[f64]),
    ) {
        let Some(filter_bytes) =
            self.header
                .len_filter_index
                .try_into()
                .ok()
                .and_then(|len: usize| {
                    usize::try_from(self.header.off_filter_index)
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
        let Some(entry_bytes) = count.checked_mul(ENTRY_A_BYTES).and_then(|len| {
            usize::try_from(self.header.off_spec_entries)
                .ok()
                .and_then(|off| {
                    off.checked_add(len)
                        .and_then(|end| self.bytes.get(off..end))
                })
        }) else {
            return;
        };

        let aref_bytes = match usize::try_from(self.header.off_spec_arrayrefs) {
            Ok(off) => self.bytes.get(off..).unwrap_or(&[]),
            Err(_) => return,
        };

        let (container, mz_buf, int_buf) = (
            &mut self.spec_container as &mut dyn ContainerAccess,
            &mut self.mz_buf,
            &mut self.int_buf,
        );

        ScanIterator::new(
            filter_bytes,
            entry_bytes,
            aref_bytes,
            container,
            mz_buf,
            int_buf,
            rt_min * 60.0,
            rt_max * 60.0,
            ms_level,
        )
        .run(callback);
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

    fn metadata_only(decoder: &Decoder<'a>) -> Result<MzML, String> {
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
                ..Default::default()
            },
        })
    }

    fn full(&mut self) -> Result<MzML, String> {
        let mut mzml = Self::metadata_only(self.decoder)?;

        if let Some(spectrum_list) = mzml.run.spectrum_list.as_mut() {
            attach_binaries(
                self.decoder.bytes,
                self.decoder.header.off_spec_entries as usize,
                self.decoder.header.off_spec_arrayrefs as usize,
                &mut spectrum_list.spectra,
                &mut self.decoder.spec_container,
                "spec",
            )?;
        }

        if let (Some(chrom_list), Some(container)) = (
            mzml.run.chromatogram_list.as_mut(),
            self.decoder.chrom_container.as_mut(),
        ) {
            attach_binaries(
                self.decoder.bytes,
                self.decoder.header.off_chrom_entries as usize,
                self.decoder.header.off_chrom_arrayrefs as usize,
                &mut chrom_list.chromatograms,
                container,
                "chrom",
            )?;
        }

        Ok(mzml)
    }
}

struct ScanIterator<'a, 'd> {
    filter_chunks: std::slice::ChunksExact<'a, u8>,
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
    fn new(
        filter_bytes: &'a [u8],
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
            filter_chunks: filter_bytes.chunks_exact(FILTER_INDEX_RECORD_SIZE),
            entry_chunks: entry_bytes.chunks_exact(ENTRY_A_BYTES),
            aref_bytes,
            container,
            mz_buf,
            int_buf,
            rt_min_s,
            rt_max_s,
            ms_level,
        }
    }

    fn run(&mut self, callback: &mut dyn FnMut(f64, &ScanMeta, &[f64], &[f64])) {
        for (filter_bytes, entry_bytes) in
            self.filter_chunks.by_ref().zip(self.entry_chunks.by_ref())
        {
            let rt_s = f64::from_le_bytes(filter_bytes[0..8].try_into().unwrap());
            if !rt_s.is_finite() || rt_s < self.rt_min_s || rt_s > self.rt_max_s {
                continue;
            }

            let ms_level = filter_bytes[40];
            if self.ms_level != 0 && ms_level != self.ms_level {
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

            let meta = ScanMeta {
                ms_level,
                polarity: filter_bytes[41],
                base_peak_mz: f64::from_le_bytes(filter_bytes[8..16].try_into().unwrap()),
                selected_ion_mz: f64::from_le_bytes(filter_bytes[16..24].try_into().unwrap()),
                base_peak_int: f64::from_le_bytes(filter_bytes[24..32].try_into().unwrap()),
                total_ion_current: f64::from_le_bytes(filter_bytes[32..40].try_into().unwrap()),
            };
            callback(
                rt_s / 60.0,
                &meta,
                &self.mz_buf[..len],
                &self.int_buf[..len],
            );
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
fn parse_filter_record(bytes: &[u8]) -> FilterRecord {
    FilterRecord {
        rt_seconds: f64::from_le_bytes(bytes[0..8].try_into().unwrap()),
        base_peak_mz: f64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        selected_ion_mz: f64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        base_peak_int: f64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        total_ion_current: f64::from_le_bytes(bytes[32..40].try_into().unwrap()),
        ms_level: bytes[40],
        polarity: bytes[41],
    }
}

#[inline]
fn read_array_refs_at(
    bytes: &[u8],
    entry_base: usize,
    aref_base: usize,
    index: usize,
) -> Option<ArrayRefs> {
    let entry_offset = index.checked_mul(ENTRY_A_BYTES)?.checked_add(entry_base)?;
    let entry_end = entry_offset.checked_add(ENTRY_A_BYTES)?;
    let entry = bytes.get(entry_offset..entry_end)?;
    let ref_start = usize::try_from(u64::from_le_bytes(entry[0..8].try_into().unwrap())).ok()?;
    let ref_count = usize::try_from(u64::from_le_bytes(entry[8..16].try_into().unwrap())).ok()?;
    let mut refs = ArrayRefs::with_capacity(ref_count);
    for offset in 0..ref_count {
        let pos = ref_start
            .checked_add(offset)?
            .checked_mul(ENTRY_A1_BYTES)?
            .checked_add(aref_base)?;
        let end = pos.checked_add(ENTRY_A1_BYTES)?;
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
    }
}

#[inline]
fn parse_array_pair(entry_bytes: &[u8], aref_bytes: &[u8]) -> Option<(ArrayRef, ArrayRef)> {
    let ref_start =
        usize::try_from(u64::from_le_bytes(entry_bytes[0..8].try_into().unwrap())).ok()?;
    let ref_count =
        usize::try_from(u64::from_le_bytes(entry_bytes[8..16].try_into().unwrap())).ok()?;
    let start = ref_start.checked_mul(ENTRY_A1_BYTES)?;
    let span = ref_count.checked_mul(ENTRY_A1_BYTES)?;
    let end = start.checked_add(span)?;
    let mut mz_ref = None;
    let mut int_ref = None;
    for bytes in aref_bytes.get(start..end)?.chunks_exact(ENTRY_A1_BYTES) {
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
    match container.get_item_from_block(
        aref.block_id,
        aref.element_offset,
        aref.element_count,
        dtype_stride(aref.dtype),
        "scan",
    ) {
        Ok(raw) => {
            decode_into(buf, raw, aref.dtype);
            true
        }
        Err(_) => false,
    }
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

#[inline]
fn reserve_exact_elements(buf: &mut Vec<f64>, len: usize) {
    if buf.capacity() < len {
        buf.reserve_exact(len - buf.capacity());
    }
}

fn decode_into(buf: &mut Vec<f64>, raw: &[u8], dtype: u8) {
    buf.clear();
    match dtype {
        1 => {
            let len = raw.len() / 8;
            reserve_exact_elements(buf, len);
            buf.extend(
                raw.chunks_exact(8)
                    .map(|chunk| f64::from_le_bytes(chunk.try_into().unwrap())),
            );
        }
        2 => {
            let len = raw.len() / 4;
            reserve_exact_elements(buf, len);
            buf.extend(
                raw.chunks_exact(4)
                    .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()) as f64),
            );
        }
        3 => {
            let len = raw.len() / 2;
            reserve_exact_elements(buf, len);
            buf.extend(
                raw.chunks_exact(2)
                    .map(|chunk| f16_bits_to_f64(u16::from_le_bytes(chunk.try_into().unwrap()))),
            );
        }
        4 => {
            let len = raw.len() / 2;
            reserve_exact_elements(buf, len);
            buf.extend(
                raw.chunks_exact(2)
                    .map(|chunk| i16::from_le_bytes(chunk.try_into().unwrap()) as f64),
            );
        }
        5 => {
            let len = raw.len() / 4;
            reserve_exact_elements(buf, len);
            buf.extend(
                raw.chunks_exact(4)
                    .map(|chunk| i32::from_le_bytes(chunk.try_into().unwrap()) as f64),
            );
        }
        6 => {
            let len = raw.len() / 8;
            reserve_exact_elements(buf, len);
            buf.extend(
                raw.chunks_exact(8)
                    .map(|chunk| i64::from_le_bytes(chunk.try_into().unwrap()) as f64),
            );
        }
        _ => {}
    }
}

fn attach_binaries<E: BinaryArrayOwner>(
    bytes: &[u8],
    entry_base: usize,
    aref_base: usize,
    entries: &mut [E],
    container: &mut dyn ContainerAccess,
    ctx: &'static str,
) -> Result<(), String> {
    for (index, entry) in entries.iter_mut().enumerate() {
        let Some(refs) = read_array_refs_at(bytes, entry_base, aref_base, index) else {
            continue;
        };
        if refs.is_empty() {
            continue;
        }
        let binary_data_array_list = entry
            .binary_data_array_list_mut()
            .get_or_insert_with(BinaryDataArrayList::default);
        for aref in refs.as_slice() {
            let raw = container.get_item_from_block(
                aref.block_id,
                aref.element_offset,
                aref.element_count,
                dtype_stride(aref.dtype),
                ctx,
            )?;
            attach_array(binary_data_array_list, aref.array_type, aref.dtype, raw);
        }
        binary_data_array_list.count = Some(binary_data_array_list.binary_data_arrays.len());
    }
    Ok(())
}

fn attach_array(bdal: &mut BinaryDataArrayList, array_type: u32, dtype: u8, raw: &[u8]) {
    let binary = raw_to_binary_data(raw, dtype);
    let numeric_type = dtype_to_numeric_type(dtype);
    let found = bdal
        .binary_data_arrays
        .iter_mut()
        .find(|array| bda_matches(array, array_type));
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
        1 => BinaryData::F64(raw_to_f64_vec(raw)),
        2 => BinaryData::F32(raw_to_f32_vec(raw)),
        3 => BinaryData::F16(raw_to_u16_vec(raw)),
        4 => BinaryData::I16(raw_to_i16_vec(raw)),
        5 => BinaryData::I32(raw_to_i32_vec(raw)),
        6 => BinaryData::I64(raw_to_i64_vec(raw)),
        _ => BinaryData::F64(Vec::new()),
    }
}

#[inline]
fn raw_to_f64_vec(raw: &[u8]) -> Vec<f64> {
    assert!(raw.len() % 8 == 0);
    let mut out = Vec::with_capacity(raw.len() / 8);
    out.extend(
        raw.chunks_exact(8)
            .map(|chunk| f64::from_le_bytes(chunk.try_into().unwrap())),
    );
    out
}

#[inline]
fn raw_to_f32_vec(raw: &[u8]) -> Vec<f32> {
    assert!(raw.len() % 4 == 0);
    let mut out = Vec::with_capacity(raw.len() / 4);
    out.extend(
        raw.chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap())),
    );
    out
}

#[inline]
fn raw_to_u16_vec(raw: &[u8]) -> Vec<u16> {
    assert!(raw.len() % 2 == 0);
    let mut out = Vec::with_capacity(raw.len() / 2);
    out.extend(
        raw.chunks_exact(2)
            .map(|chunk| u16::from_le_bytes(chunk.try_into().unwrap())),
    );
    out
}

#[inline]
fn raw_to_i16_vec(raw: &[u8]) -> Vec<i16> {
    assert!(raw.len() % 2 == 0);
    let mut out = Vec::with_capacity(raw.len() / 2);
    out.extend(
        raw.chunks_exact(2)
            .map(|chunk| i16::from_le_bytes(chunk.try_into().unwrap())),
    );
    out
}

#[inline]
fn raw_to_i32_vec(raw: &[u8]) -> Vec<i32> {
    assert!(raw.len() % 4 == 0);
    let mut out = Vec::with_capacity(raw.len() / 4);
    out.extend(
        raw.chunks_exact(4)
            .map(|chunk| i32::from_le_bytes(chunk.try_into().unwrap())),
    );
    out
}

#[inline]
fn raw_to_i64_vec(raw: &[u8]) -> Vec<i64> {
    assert!(raw.len() % 8 == 0);
    let mut out = Vec::with_capacity(raw.len() / 8);
    out.extend(
        raw.chunks_exact(8)
            .map(|chunk| i64::from_le_bytes(chunk.try_into().unwrap())),
    );
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

fn make_bda_stub(acc: u32) -> BinaryDataArray {
    BinaryDataArray {
        cv_params: vec![CvParam {
            cv_ref: Some("MS".to_string()),
            accession: Some(format!("MS:{acc:07}")),
            name: String::new(),
            ..Default::default()
        }],
        ..BinaryDataArray::default()
    }
}

#[inline]
fn sync_numeric_meta(bda: &mut BinaryDataArray, nt: NumericType) {
    let target = match nt {
        NumericType::Float16 => 1_000_520,
        NumericType::Float32 => 1_000_521,
        NumericType::Float64 => 1_000_523,
        NumericType::Int16 => 1_000_518,
        NumericType::Int32 => 1_000_519,
        NumericType::Int64 => 1_000_522,
    };
    bda.cv_params
        .retain(|param| !is_numeric_acc(parse_accession_tail(param.accession.as_deref())));
    bda.cv_params.push(CvParam {
        cv_ref: Some("MS".into()),
        accession: Some(format!("MS:{target:07}")),
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
    bda.numeric_type = Some(nt);
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
        .any(|param| parse_accession_tail(param.accession.as_deref()).raw() == kind)
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
        .filter_map(|&id| {
            get_attr_text(owner_rows.get(id), ACC_ATTR_REF)
                .map(|value| SourceFileRef { r#ref: value })
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

    const BYTES: &[u8] = include_bytes!("../../../data/ion/test.ion");

    #[test]
    fn open_parses_header() {
        let d = Decoder::open(BYTES).unwrap();
        assert!(d.spectrum_count() > 0);
    }

    #[test]
    fn filter_record_returns_none_out_of_bounds() {
        let d = Decoder::open(BYTES).unwrap();
        assert!(d.filter_record(d.spectrum_count() as usize).is_none());
    }

    #[test]
    fn filter_record_has_valid_rt() {
        let d = Decoder::open(BYTES).unwrap();
        let r = d.filter_record(0).unwrap();
        assert!(r.rt_seconds.is_finite() && r.rt_seconds >= 0.0);
        assert!(r.ms_level >= 1);
    }

    #[test]
    fn array_refs_contain_mz_and_intensity() {
        let d = Decoder::open(BYTES).unwrap();
        let refs = d.spectrum_array_refs(0).unwrap();
        assert!(refs.iter().any(|a| a.array_type == ACC_MZ));
        assert!(refs.iter().any(|a| a.array_type == ACC_INT));
    }

    #[test]
    fn read_spectrum_array_produces_mz_values() {
        let mut d = Decoder::open(BYTES).unwrap();
        let refs = d.spectrum_array_refs(0).unwrap();
        let mz_ref = refs.iter().find(|a| a.array_type == ACC_MZ).unwrap();

        let mut mz = Vec::new();
        d.read_spectrum_array(mz_ref, &mut mz).unwrap();

        assert!(!mz.is_empty());
        assert!(mz.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn for_each_scan_yields_matching_scans() {
        let mut d = Decoder::open(BYTES).unwrap();
        let mut count = 0usize;
        d.for_each_scan_in_range(0.0, f64::MAX, 0, &mut |rt, _, mz, int| {
            assert!(rt.is_finite());
            assert!(!mz.is_empty());
            assert_eq!(mz.len(), int.len());
            count += 1;
        });
        assert_eq!(count, d.spectrum_count() as usize);
    }

    #[test]
    fn for_each_scan_filters_by_ms_level() {
        let mut d = Decoder::open(BYTES).unwrap();
        let mut count = 0usize;
        d.for_each_scan_in_range(0.0, f64::MAX, 1, &mut |_, _, _, _| {
            count += 1;
        });
        let expected = (0..d.spectrum_count() as usize)
            .filter(|&i| d.filter_record(i).map_or(false, |r| r.ms_level == 1))
            .count();
        assert_eq!(count, expected);
    }

    #[allow(deprecated)]
    #[test]
    fn to_mzml_produces_valid_structure() {
        let mut d = Decoder::open(BYTES).unwrap();
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
        let d = Decoder::open(BYTES).unwrap();
        assert!(!d.global_metadata().unwrap().is_empty());
    }

    #[test]
    fn spectrum_metadata_returns_entries() {
        let d = Decoder::open(BYTES).unwrap();
        assert!(!d.spectrum_metadata().unwrap().is_empty());
    }

    #[test]
    fn custom_config_opens_successfully() {
        let config = DecoderConfig {
            max_cached_bytes: 1024 * 1024,
        };
        let d = Decoder::open_with_config(BYTES, config).unwrap();
        assert!(d.spectrum_count() > 0);
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
        let mut ion = Ion::open(BYTES).unwrap();
        ion.load_metadata().unwrap();
        assert!(ion.spectrum_count() > 0);
        assert!(ion.run.spectrum_list.is_some());
        assert!(ion.software_list.is_some() || ion.instrument_list.is_some());
    }

    #[test]
    fn ion_for_each_scan_yields_scans() {
        let mut ion = Ion::open(BYTES).unwrap();
        let mut count = 0usize;
        ion.for_each_scan_in_range(0.0, f64::MAX, 0, &mut |rt, _, mz, int| {
            assert!(rt.is_finite());
            assert!(!mz.is_empty());
            assert_eq!(mz.len(), int.len());
            count += 1;
        });
        assert_eq!(count, ion.spectrum_count() as usize);
    }

    #[test]
    fn ion_filter_record_matches_decoder() {
        let ion = Ion::open(BYTES).unwrap();
        let r = ion.filter_record(0).unwrap();
        assert!(r.rt_seconds.is_finite());
        assert!(r.ms_level >= 1);
    }

    #[test]
    fn ion_to_mzml_has_binaries() {
        let mut ion = Ion::open(BYTES).unwrap();
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
}
