use crate::{
    accessions::{MZ_ARRAY, TIME_ARRAY},
    encoder::utilities::{
        FileHeader,
        encoder_output::EncoderOutput,
        meta_collector::{
            ArrayPolicy, MetaCollector, MetaGrouper, compress_bytes_if_enabled,
            serialize_global_meta_with_counts,
        },
        tables::{ArrayRefTable, IndexTable, SummaryTable, write_aligned},
    },
    ion::{
        IonResult,
        encoder::{
            encode::{
                CHROM_SUMMARY_SIZE, EncodingConfig, SPEC_SUMMARY_SIZE, encode_single_array,
                extract_chrom_summary, spec_summary_from_spectrum,
            },
            utilities::{ContainerBuilder, DefaultCompressor},
        },
        format::{FILE_TRAILER, HEADER_SIZE},
        meta_groups::METADATA_GROUP_SIZE,
    },
    mzml::structs::{BinaryDataArrayList, Chromatogram, MzML, Spectrum},
};

fn spec_summary_bytes(spec: &Spectrum) -> [u8; SPEC_SUMMARY_SIZE] {
    let s = spec_summary_from_spectrum(spec);
    let mut buf = [0u8; SPEC_SUMMARY_SIZE];
    buf[0..8].copy_from_slice(&s.rt_seconds.to_le_bytes());
    buf[8..16].copy_from_slice(&s.base_peak_mz.to_le_bytes());
    buf[16..24].copy_from_slice(&s.selected_ion_mz.to_le_bytes());
    buf[24..32].copy_from_slice(&s.base_peak_int.to_le_bytes());
    buf[32..40].copy_from_slice(&s.total_ion_current.to_le_bytes());
    buf[40] = s.ms_level;
    buf[41] = s.polarity;
    buf
}

fn chrom_summary_bytes(chrom: &Chromatogram) -> [u8; CHROM_SUMMARY_SIZE] {
    let s = extract_chrom_summary(chrom);
    let mut buf = [0u8; CHROM_SUMMARY_SIZE];
    buf[0..8].copy_from_slice(&s.lowest_mz.to_le_bytes());
    buf[8..16].copy_from_slice(&s.highest_mz.to_le_bytes());
    buf[16..24].copy_from_slice(&s.lowest_wavelength.to_le_bytes());
    buf[24..32].copy_from_slice(&s.highest_wavelength.to_le_bytes());
    buf[32..40].copy_from_slice(&s.lowest_ion_mobility.to_le_bytes());
    buf[40..48].copy_from_slice(&s.highest_ion_mobility.to_le_bytes());
    buf[48] = s.polarity;
    buf
}

fn encode_arrays_for<T>(
    item: &T,
    config: EncodingConfig,
    policy: ArrayPolicy,
    container: &mut ContainerBuilder<'_, DefaultCompressor>,
    index: &mut IndexTable,
    arefs: &mut ArrayRefTable,
    cursor: &mut u64,
    seen: &mut Vec<u32>,
) -> IonResult<()>
where
    T: HasArrayList,
{
    let aref_start = *cursor;
    let mut aref_count: u64 = 0;

    if let Some(list) = item.array_list() {
        for bda in &list.binary_data_arrays {
            let Some(aref) = encode_single_array(bda, config, policy, container)? else {
                continue;
            };
            if aref.accession != 0 && !seen.contains(&aref.accession) {
                seen.push(aref.accession);
            }
            arefs.push(
                aref.element_offset,
                aref.element_count,
                aref.block_id,
                aref.accession,
                aref.dtype,
                aref.array_filter,
                aref.encoded_len,
            );
            *cursor += 1;
            aref_count += 1;
        }
    }
    index.push(aref_start, aref_count);
    Ok(())
}

trait HasArrayList {
    fn array_list(&self) -> Option<&BinaryDataArrayList>;
}

impl HasArrayList for Spectrum {
    fn array_list(&self) -> Option<&BinaryDataArrayList> {
        self.binary_data_array_list.as_ref()
    }
}

impl HasArrayList for Chromatogram {
    fn array_list(&self) -> Option<&BinaryDataArrayList> {
        self.binary_data_array_list.as_ref()
    }
}

pub struct IonWriter<'out> {
    output: &'out mut dyn EncoderOutput,
    config: EncodingConfig,
    collector: MetaCollector,
    spec_list_id: u32,
    chrom_list_id: u32,
    spec_grouper: MetaGrouper,
    chrom_grouper: MetaGrouper,
    spec_summary: SummaryTable,
    chrom_summary: SummaryTable,
    spec_index: IndexTable,
    chrom_index: IndexTable,
    spec_arefs: ArrayRefTable,
    chrom_arefs: ArrayRefTable,
    spec_count: usize,
    chrom_count: usize,
    spec_aref_cursor: u64,
    chrom_aref_cursor: u64,
    spec_seen: Vec<u32>,
    chrom_seen: Vec<u32>,
    spec_container_offset: u64,
    spec_block_count: u64,
    spec_container_total: u64,
    chrom_container_offset: u64,
    chrom_block_count: u64,
    chrom_container_total: u64,
}

impl<'out> IonWriter<'out> {
    pub fn begin(output: &'out mut dyn EncoderOutput, config: EncodingConfig) -> IonResult<Self> {
        output.write_bytes(&[0u8; HEADER_SIZE])?;

        let mut collector = MetaCollector::new();
        let spec_list_id = collector.alloc();
        let chrom_list_id = collector.alloc();

        let group_size = METADATA_GROUP_SIZE;
        let level = config.compression_level;

        Ok(Self {
            output,
            config,
            collector,
            spec_list_id,
            chrom_list_id,
            spec_grouper: MetaGrouper::new(group_size, level),
            chrom_grouper: MetaGrouper::new(group_size, level),
            spec_summary: SummaryTable::new(256, SPEC_SUMMARY_SIZE),
            chrom_summary: SummaryTable::new(32, CHROM_SUMMARY_SIZE),
            spec_index: IndexTable::new(256),
            chrom_index: IndexTable::new(32),
            spec_arefs: ArrayRefTable::new(256),
            chrom_arefs: ArrayRefTable::new(32),
            spec_count: 0,
            chrom_count: 0,
            spec_aref_cursor: 0,
            chrom_aref_cursor: 0,
            spec_seen: Vec::with_capacity(8),
            chrom_seen: Vec::with_capacity(8),
            spec_container_offset: 0,
            spec_block_count: 0,
            spec_container_total: 0,
            chrom_container_offset: 0,
            chrom_block_count: 0,
            chrom_container_total: 0,
        })
    }

    pub fn write_mzml(&mut self, mzml: &MzML) -> IonResult<()> {
        let spectra = mzml
            .run
            .spectrum_list
            .as_ref()
            .map_or(&[][..], |sl| &sl.spectra);
        let chroms = mzml
            .run
            .chromatogram_list
            .as_ref()
            .map_or(&[][..], |cl| &cl.chromatograms);

        let spec_policy = self.config.array_policy(MZ_ARRAY);
        let chrom_policy = self.config.array_policy(TIME_ARRAY);

        let spec_list_id = self.spec_list_id;
        let chrom_list_id = self.chrom_list_id;

        let spec_container_offset = write_aligned(self.output, &[])?;
        self.spec_container_offset = spec_container_offset;

        {
            let compressor = self.config.compression_mode();
            let builder = ContainerBuilder::new(
                self.output,
                self.config.uncompressed_block_size,
                compressor,
                self.config.block_packing_id(),
            );
            let mut spec_container = if self.config.parallel {
                builder
            } else {
                builder.force_sequential()
            };

            let spec_list_schema = mzml.run.spectrum_list.as_ref();
            for (i, spec) in spectra.iter().enumerate() {
                encode_arrays_for(
                    spec,
                    self.config,
                    spec_policy,
                    &mut spec_container,
                    &mut self.spec_index,
                    &mut self.spec_arefs,
                    &mut self.spec_aref_cursor,
                    &mut self.spec_seen,
                )?;
                self.spec_summary.push(&spec_summary_bytes(spec));
                self.collector.add_item(
                    spec,
                    i,
                    spec_list_id,
                    spec_list_schema,
                    spec_policy,
                    &mut self.spec_grouper,
                );
                self.spec_count += 1;
            }

            let (block_count, total) = spec_container.finish()?;
            self.spec_block_count = block_count as u64;
            self.spec_container_total = total;
        }
        let chrom_container_offset = write_aligned(self.output, &[])?;
        self.chrom_container_offset = chrom_container_offset;

        {
            let compressor = self.config.compression_mode();
            let builder = ContainerBuilder::new(
                self.output,
                self.config.uncompressed_block_size,
                compressor,
                self.config.block_packing_id(),
            );
            let mut chrom_container = if self.config.parallel {
                builder
            } else {
                builder.force_sequential()
            };

            let chrom_list_schema = mzml.run.chromatogram_list.as_ref();
            for (i, chrom) in chroms.iter().enumerate() {
                encode_arrays_for(
                    chrom,
                    self.config,
                    chrom_policy,
                    &mut chrom_container,
                    &mut self.chrom_index,
                    &mut self.chrom_arefs,
                    &mut self.chrom_aref_cursor,
                    &mut self.chrom_seen,
                )?;
                self.chrom_summary.push(&chrom_summary_bytes(chrom));
                self.collector.add_item(
                    chrom,
                    i,
                    chrom_list_id,
                    chrom_list_schema,
                    chrom_policy,
                    &mut self.chrom_grouper,
                );
                self.chrom_count += 1;
            }

            let (block_count, total) = chrom_container.finish()?;
            self.chrom_block_count = block_count as u64;
            self.chrom_container_total = total;
        }

        self.finish_inner(mzml)
    }

    fn finish_inner(&mut self, mzml: &MzML) -> IonResult<()> {
        let (global_meta, global_counts) = self.collector.collect_global_meta(mzml);
        let raw_global = serialize_global_meta_with_counts(&global_counts, &global_meta);
        let global_uncompressed = raw_global.len() as u64;
        let global_bytes = compress_bytes_if_enabled(raw_global, self.config.compression_level);

        let spec_grouped = std::mem::replace(
            &mut self.spec_grouper,
            MetaGrouper::new(METADATA_GROUP_SIZE, self.config.compression_level),
        )
        .finish();
        let chrom_grouped = std::mem::replace(
            &mut self.chrom_grouper,
            MetaGrouper::new(METADATA_GROUP_SIZE, self.config.compression_level),
        )
        .finish();

        let spec_summary_bytes_vec = std::mem::take(&mut self.spec_summary).finish();
        let chrom_summary_bytes_vec = std::mem::take(&mut self.chrom_summary).finish();
        let spec_index_bytes = std::mem::take(&mut self.spec_index).finish();
        let chrom_index_bytes = std::mem::take(&mut self.chrom_index).finish();
        let spec_aref_bytes = std::mem::take(&mut self.spec_arefs).finish();
        let chrom_aref_bytes = std::mem::take(&mut self.chrom_arefs).finish();

        let spec_meta_crc32 = crc32fast::hash(&spec_grouped.bytes);
        let chrom_meta_crc32 = crc32fast::hash(&chrom_grouped.bytes);
        let global_meta_crc32 = crc32fast::hash(&global_bytes);

        let off_spec_summary = write_aligned(self.output, &spec_summary_bytes_vec)?;
        let off_spec_entries = write_aligned(self.output, &spec_index_bytes)?;
        let off_spec_arrayrefs = write_aligned(self.output, &spec_aref_bytes)?;
        let off_chrom_summary = write_aligned(self.output, &chrom_summary_bytes_vec)?;
        let off_chrom_entries = write_aligned(self.output, &chrom_index_bytes)?;
        let off_chrom_arrayrefs = write_aligned(self.output, &chrom_aref_bytes)?;
        let off_spec_meta = write_aligned(self.output, &spec_grouped.bytes)?;
        let off_chrom_meta = write_aligned(self.output, &chrom_grouped.bytes)?;
        let off_global_meta = write_aligned(self.output, &global_bytes)?;

        self.output.write_bytes(&FILE_TRAILER)?;
        let total_file_size = self.output.current_byte_position()?;

        let header = FileHeader {
            compression_codec: self.config.codec_id(),
            compression_level: self.config.compression_level,
            array_filter_id: self.config.array_filter_id(),
            target_block_size: self.config.uncompressed_block_size as u64,

            offset_spec_entries: off_spec_entries,
            len_spec_entries: spec_index_bytes.len() as u64,
            offset_spec_arrayrefs: off_spec_arrayrefs,
            len_spec_arrayrefs: spec_aref_bytes.len() as u64,
            offset_chrom_entries: off_chrom_entries,
            len_chrom_entries: chrom_index_bytes.len() as u64,
            offset_chrom_arrayrefs: off_chrom_arrayrefs,
            len_chrom_arrayrefs: chrom_aref_bytes.len() as u64,
            offset_spec_meta: off_spec_meta,
            len_spec_meta: spec_grouped.bytes.len() as u64,
            offset_chrom_meta: off_chrom_meta,
            len_chrom_meta: chrom_grouped.bytes.len() as u64,
            offset_global_meta: off_global_meta,
            len_global_meta: global_bytes.len() as u64,
            offset_packed_spectra: self.spec_container_offset,
            len_packed_spectra: self.spec_container_total,
            offset_packed_chroms: self.chrom_container_offset,
            len_packed_chroms: self.chrom_container_total,

            spectrum_block_count: self.spec_block_count,
            chrom_block_count: self.chrom_block_count,
            spectrum_count: self.spec_count as u64,
            chrom_count: self.chrom_count as u64,

            spec_meta_row_count: spec_grouped.row_count,
            spec_meta_numeric_count: spec_grouped.numeric_count,
            spec_meta_string_count: spec_grouped.string_count,
            chrom_meta_row_count: chrom_grouped.row_count,
            chrom_meta_numeric_count: chrom_grouped.numeric_count,
            chrom_meta_string_count: chrom_grouped.string_count,
            global_meta_row_count: global_meta.ref_codes.len() as u64,
            global_meta_numeric_count: global_meta.numeric_values.len() as u64,
            global_meta_string_count: global_meta.string_offsets.len() as u64,
            spec_array_type_count: self.spec_seen.len() as u64,
            chrom_array_type_count: self.chrom_seen.len() as u64,

            spec_meta_uncompressed_size: spec_grouped.uncompressed_size,
            chrom_meta_uncompressed_size: chrom_grouped.uncompressed_size,
            global_meta_uncompressed_size: global_uncompressed,

            meta_group_size: METADATA_GROUP_SIZE,
            spec_meta_group_count: spec_grouped.group_count,
            chrom_meta_group_count: chrom_grouped.group_count,

            off_spec_summary,
            len_spec_summary: spec_summary_bytes_vec.len() as u64,
            off_chrom_summary,
            len_chrom_summary: chrom_summary_bytes_vec.len() as u64,

            total_file_size,

            spec_meta_crc32,
            chrom_meta_crc32,
            global_meta_crc32,
            header_crc32: 0,
        };

        let mut header_bytes = [0u8; HEADER_SIZE];
        header.write_into(&mut header_bytes);
        let crc = crc32fast::hash(&header_bytes[0..1020]);
        header_bytes[1020..1024].copy_from_slice(&crc.to_le_bytes());
        self.output.patch_bytes_at(0, &header_bytes)
    }
}

pub fn write_mzml_to_ion(
    mzml: &MzML,
    config: EncodingConfig,
    output: &mut dyn EncoderOutput,
) -> IonResult<()> {
    if config.compression_level > 22 {
        return Err(format!(
            "compression_level must be 0–22, got {}",
            config.compression_level
        )
        .into());
    }
    IonWriter::begin(output, config)?.write_mzml(mzml)
}
