use std::borrow::Borrow;

pub(crate) struct SectionPlacement {
    pub(crate) offset: u64,
    pub(crate) length: u64,
    pub(crate) plain_len: u64,
    pub(crate) crc32: u32,
}

use crate::{
    accessions::{MZ_ARRAY, TIME_ARRAY},
    encoder::file_reader::FileReader,
    encoder::utilities::{
        FileHeader, SectionChunk,
        encoder_output::EncoderOutput,
        make_chunk,
        meta_collector::{
            ArrayPolicy, GroupedSection, LOCAL_LIST_NODE_ID, MetaCollector, MetaGrouper,
            MzmlListItem, array_type_accession_from_binary_data_array, compress_bytes_if_enabled,
            serialize_global_meta_with_counts,
        },
        segments::{SegmentPlan, allow_split, get_segment_ranges},
        tables::{
            ArrayRefTable, IndexTable, SegmentBound, SegmentBoundsTable, SummaryTable,
            write_aligned,
        },
    },
    ion::{
        IonResult,
        encoder::{
            encode::{
                CHROM_SUMMARY_SIZE, EncodedArrayRef, EncodingConfig, SPEC_SUMMARY_SIZE,
                allow_compression_level, array_is_fixed_width_splittable, encode_single_array,
                extract_chrom_summary, get_segment_bounds, spec_summary_from_spectrum,
                write_array_segments,
            },
            utilities::{ContainerBuilder, DefaultCompressor},
        },
        format::{FILE_TRAILER, HEADER_SIZE},
        meta_groups::METADATA_GROUP_SIZE,
        utilities::EmitAttributes,
    },
    mzml::structs::{BinaryDataArray, BinaryDataArrayList, Chromatogram, MzML, Spectrum},
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
    buf[42..46].copy_from_slice(&s.position_x.to_le_bytes());
    buf[46..50].copy_from_slice(&s.position_y.to_le_bytes());
    buf[50..54].copy_from_slice(&s.position_z.to_le_bytes());
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

fn allow_split_for_item(
    arrays: &[BinaryDataArray],
    config: EncodingConfig,
    policy: ArrayPolicy,
) -> IonResult<Option<SegmentPlan>> {
    let Some(x_array) = arrays
        .iter()
        .find(|bda| array_type_accession_from_binary_data_array(bda) == policy.x_array_accession)
    else {
        return Ok(None);
    };

    let Some((x_count, x_elem_bytes)) = array_is_fixed_width_splittable(x_array, config, policy)?
    else {
        return Ok(None);
    };
    if x_count == 0 {
        return Ok(None);
    }

    for bda in arrays {
        match array_is_fixed_width_splittable(bda, config, policy)? {
            Some((count, _)) if count == x_count => {}
            _ => return Ok(None),
        }
    }

    let plan = get_segment_ranges(x_count, x_elem_bytes, config.target_segment_bytes);
    let x_array_bytes = x_count * x_elem_bytes;
    if !allow_split(x_array_bytes, config.min_split_bytes, &plan) {
        return Ok(None);
    }
    Ok(Some(plan))
}

struct ArrayWriteState<'a> {
    arefs: &'a mut ArrayRefTable,
    cursor: &'a mut u64,
    seen: &'a mut Vec<u32>,
    segment_bounds: &'a mut SegmentBoundsTable,
}

impl ArrayWriteState<'_> {
    fn emit(&mut self, aref: &EncodedArrayRef) -> IonResult<()> {
        if aref.accession != 0 && !self.seen.contains(&aref.accession) {
            self.seen.push(aref.accession);
        }
        self.arefs.push(
            aref.element_offset,
            aref.element_count,
            aref.block_id,
            aref.accession,
            aref.dtype,
            aref.array_filter,
            aref.encoded_len,
            aref.continues_previous_segment,
        )?;
        *self.cursor += 1;
        Ok(())
    }
}

fn write_single_array(
    bda: &BinaryDataArray,
    config: EncodingConfig,
    policy: ArrayPolicy,
    container: &mut ContainerBuilder<'_, DefaultCompressor>,
    state: &mut ArrayWriteState<'_>,
) -> IonResult<()> {
    use crate::ion::encoder::encode::get_array_bounds;

    let Some(aref) = encode_single_array(bda, config, policy, container)? else {
        return Ok(());
    };
    let ref_index = *state.cursor;
    state.emit(&aref)?;

    if let Some((low, high)) = get_array_bounds(bda, aref.dtype) {
        state.segment_bounds.push(SegmentBound {
            array_ref_index: ref_index,
            low,
            high,
        })?;
    }
    Ok(())
}

fn write_split_array(
    bda: &BinaryDataArray,
    config: EncodingConfig,
    policy: ArrayPolicy,
    plan: &SegmentPlan,
    container: &mut ContainerBuilder<'_, DefaultCompressor>,
    state: &mut ArrayWriteState<'_>,
) -> IonResult<()> {
    let refs = write_array_segments(bda, config, policy, plan, container)?;
    let Some(first) = refs.first() else {
        return Ok(());
    };
    let bounds = get_segment_bounds(bda, plan, first.dtype);
    for (segment_index, aref) in refs.iter().enumerate() {
        let ref_index = *state.cursor;
        state.emit(aref)?;
        if let Some(bounds) = bounds.as_ref() {
            let (low, high) = bounds[segment_index];
            state.segment_bounds.push(SegmentBound {
                array_ref_index: ref_index,
                low,
                high,
            })?;
        }
    }
    Ok(())
}

fn encode_arrays_for<T>(
    item: &T,
    config: EncodingConfig,
    policy: ArrayPolicy,
    allow_splitting: bool,
    container: &mut ContainerBuilder<'_, DefaultCompressor>,
    index: &mut IndexTable,
    state: &mut ArrayWriteState<'_>,
) -> IonResult<()>
where
    T: HasArrayList,
{
    let aref_start = *state.cursor;

    if let Some(list) = item.array_list() {
        let plan = if allow_splitting {
            allow_split_for_item(&list.binary_data_arrays, config, policy)?
        } else {
            None
        };

        for bda in &list.binary_data_arrays {
            match plan.as_ref() {
                Some(plan) => write_split_array(bda, config, policy, plan, container, state)?,
                None => write_single_array(bda, config, policy, container, state)?,
            }
        }
    }

    let aref_count = *state.cursor - aref_start;
    index.push(aref_start, aref_count)?;
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

struct ItemStream {
    summary: SummaryTable,
    index: IndexTable,
    arefs: ArrayRefTable,
    grouper: MetaGrouper,
    segment_bounds: SegmentBoundsTable,
    allow_splitting: bool,
    count: usize,
    aref_cursor: u64,
    seen: Vec<u32>,
    container_offset: u64,
    block_count: u64,
    container_total: u64,
    directory_crc32: u32,
}

struct StreamParts {
    grouped: GroupedSection,
    summary: SectionChunk,
    index: SectionChunk,
    arefs: SectionChunk,
    segment_bounds: SectionChunk,
    count: usize,
    type_count: usize,
    container_offset: u64,
    block_count: u64,
    container_total: u64,
    directory_crc32: u32,
}

impl ItemStream {
    fn new(
        tag: &str,
        summary_hint: usize,
        summary_size: usize,
        table_hint: usize,
        config: EncodingConfig,
        allow_splitting: bool,
    ) -> IonResult<Self> {
        let mode = config.section_chunk;
        Ok(Self {
            summary: SummaryTable::new(make_chunk(
                mode,
                &format!("{tag}-summary"),
                summary_hint * summary_size,
            )?),
            index: IndexTable::new(make_chunk(mode, &format!("{tag}-index"), table_hint * 16)?),
            arefs: ArrayRefTable::new(make_chunk(
                mode,
                &format!("{tag}-arrayrefs"),
                table_hint * 64,
            )?),
            grouper: MetaGrouper::new(
                METADATA_GROUP_SIZE,
                config.compression_level,
                make_chunk(mode, &format!("{tag}-meta"), 0)?,
            ),
            segment_bounds: SegmentBoundsTable::new(make_chunk(
                mode,
                &format!("{tag}-segment-bounds"),
                0,
            )?),
            allow_splitting,
            count: 0,
            aref_cursor: 0,
            seen: Vec::with_capacity(8),
            container_offset: 0,
            block_count: 0,
            container_total: 0,
            directory_crc32: 0,
        })
    }

    #[allow(clippy::too_many_arguments)] //TODO: Need to fix this
    fn add<T, L>(
        &mut self,
        item: &T,
        config: EncodingConfig,
        policy: ArrayPolicy,
        list_id: u32,
        list_schema: Option<&L>,
        collector: &mut MetaCollector,
        container: &mut ContainerBuilder<'_, DefaultCompressor>,
        summary: &[u8],
    ) -> IonResult<()>
    where
        T: HasArrayList + MzmlListItem,
        L: EmitAttributes,
    {
        let mut state = ArrayWriteState {
            arefs: &mut self.arefs,
            cursor: &mut self.aref_cursor,
            seen: &mut self.seen,
            segment_bounds: &mut self.segment_bounds,
        };
        encode_arrays_for(
            item,
            config,
            policy,
            self.allow_splitting,
            container,
            &mut self.index,
            &mut state,
        )?;
        self.summary.push(summary)?;
        collector.add_item(
            item,
            self.count,
            list_id,
            list_schema,
            policy,
            &mut self.grouper,
        )?;
        self.count += 1;
        Ok(())
    }

    fn finish(self) -> IonResult<StreamParts> {
        Ok(StreamParts {
            grouped: self.grouper.finish()?,
            summary: self.summary.finish(),
            index: self.index.finish(),
            arefs: self.arefs.finish(),
            segment_bounds: self.segment_bounds.finish(),
            count: self.count,
            type_count: self.seen.len(),
            container_offset: self.container_offset,
            block_count: self.block_count,
            container_total: self.container_total,
            directory_crc32: self.directory_crc32,
        })
    }
}

fn write_chunk(output: &mut dyn EncoderOutput, section: SectionChunk) -> IonResult<(u64, u64)> {
    let byte_len = section.len();
    let offset = section.copy_into(output)?;
    Ok((offset, byte_len))
}

#[allow(clippy::too_many_arguments)]
fn write_list<T, L, B, I>(
    output: &mut dyn EncoderOutput,
    config: EncodingConfig,
    policy: ArrayPolicy,
    list_id: u32,
    list_schema: Option<&L>,
    collector: &mut MetaCollector,
    stream: &mut ItemStream,
    items: I,
    summary_of: fn(&T) -> [u8; SPEC_SUMMARY_SIZE],
) -> IonResult<()>
where
    T: HasArrayList + MzmlListItem,
    L: EmitAttributes,
    B: Borrow<T>,
    I: Iterator<Item = IonResult<B>>,
{
    stream.container_offset = write_aligned(output, &[])?;
    let compressor = config.compression_mode()?;
    let builder = ContainerBuilder::new(
        output,
        config.uncompressed_block_size,
        compressor,
        config.block_packing_id(),
    );
    let mut container = if config.parallel {
        builder
    } else {
        builder.force_sequential()
    };

    for item in items {
        let item = item?;
        let item = item.borrow();
        let summary = summary_of(item);
        stream.add(
            item,
            config,
            policy,
            list_id,
            list_schema,
            collector,
            &mut container,
            &summary,
        )?;
    }

    let summary = container.finish()?;
    stream.block_count = summary.block_count as u64;
    stream.container_total = summary.total_bytes;
    stream.directory_crc32 = summary.directory_crc32;
    Ok(())
}

pub struct IonWriter<'out> {
    output: &'out mut dyn EncoderOutput,
    config: EncodingConfig,
    collector: MetaCollector,
    spec_list_id: u32,
    chrom_list_id: u32,
    spec_stream: ItemStream,
    chrom_stream: ItemStream,
}

impl<'out> IonWriter<'out> {
    pub fn begin(output: &'out mut dyn EncoderOutput, config: EncodingConfig) -> IonResult<Self> {
        allow_compression_level(config.compression_level)?;
        output.write_bytes(&[0u8; HEADER_SIZE])?;

        let collector = MetaCollector::new();
        let spec_list_id = LOCAL_LIST_NODE_ID;
        let chrom_list_id = LOCAL_LIST_NODE_ID;

        Ok(Self {
            output,
            config,
            collector,
            spec_list_id,
            chrom_list_id,
            spec_stream: ItemStream::new("spec", 256, SPEC_SUMMARY_SIZE, 256, config, true)?,
            chrom_stream: ItemStream::new("chrom", 32, CHROM_SUMMARY_SIZE, 32, config, false)?,
        })
    }

    pub fn write_mzml(&mut self, mzml: &MzML) -> IonResult<()> {
        let spectra = mzml
            .run
            .spectrum_list
            .as_ref()
            .map_or(&[][..], |list| &list.spectra);
        let chroms = mzml
            .run
            .chromatogram_list
            .as_ref()
            .map_or(&[][..], |list| &list.chromatograms);

        write_list(
            self.output,
            self.config,
            self.config.array_policy(MZ_ARRAY),
            self.spec_list_id,
            mzml.run.spectrum_list.as_ref(),
            &mut self.collector,
            &mut self.spec_stream,
            spectra.iter().map(Ok),
            spec_summary_bytes,
        )?;
        write_list(
            self.output,
            self.config,
            self.config.array_policy(TIME_ARRAY),
            self.chrom_list_id,
            mzml.run.chromatogram_list.as_ref(),
            &mut self.collector,
            &mut self.chrom_stream,
            chroms.iter().map(Ok),
            chrom_summary_bytes,
        )?;

        self.finish_inner(mzml)
    }

    pub fn write_reader(&mut self, reader: &mut dyn FileReader) -> IonResult<()> {
        let metadata = reader.get_metadata()?;
        write_list(
            self.output,
            self.config,
            self.config.array_policy(MZ_ARRAY),
            self.spec_list_id,
            metadata.run.spectrum_list.as_ref(),
            &mut self.collector,
            &mut self.spec_stream,
            std::iter::from_fn(|| reader.next_spectrum().transpose()),
            spec_summary_bytes,
        )?;

        let metadata = reader.get_metadata()?;
        write_list(
            self.output,
            self.config,
            self.config.array_policy(TIME_ARRAY),
            self.chrom_list_id,
            metadata.run.chromatogram_list.as_ref(),
            &mut self.collector,
            &mut self.chrom_stream,
            std::iter::from_fn(|| reader.next_chromatogram().transpose()),
            chrom_summary_bytes,
        )?;

        let metadata = reader.get_metadata()?;
        self.finish_inner(&metadata)
    }

    fn write_segment_bounds(&mut self, bounds: SectionChunk) -> IonResult<SectionPlacement> {
        let raw = bounds.into_vec()?;
        let plain_len = raw.len() as u64;
        if raw.is_empty() {
            return Ok(SectionPlacement {
                offset: 0,
                length: 0,
                plain_len: 0,
                crc32: crc32fast::hash(&[]),
            });
        }
        let stored = compress_bytes_if_enabled(raw, self.config.compression_level);
        let crc32 = crc32fast::hash(&stored);
        let offset = write_aligned(self.output, &stored)?;
        let length = stored.len() as u64;
        Ok(SectionPlacement {
            offset,
            length,
            plain_len,
            crc32,
        })
    }

    fn finish_inner(&mut self, mzml: &MzML) -> IonResult<()> {
        let (global_meta, global_counts) = self.collector.collect_global_meta(mzml);
        let raw_global = serialize_global_meta_with_counts(&global_counts, &global_meta);
        let global_uncompressed = raw_global.len() as u64;
        let global_bytes = compress_bytes_if_enabled(raw_global, self.config.compression_level);

        let spec = std::mem::replace(
            &mut self.spec_stream,
            ItemStream::new("spec", 256, SPEC_SUMMARY_SIZE, 256, self.config, true)?,
        )
        .finish()?;
        let chrom = std::mem::replace(
            &mut self.chrom_stream,
            ItemStream::new("chrom", 32, CHROM_SUMMARY_SIZE, 32, self.config, false)?,
        )
        .finish()?;

        let spec_meta_crc32 = spec.grouped.crc32;
        let chrom_meta_crc32 = chrom.grouped.crc32;
        let global_meta_crc32 = crc32fast::hash(&global_bytes);

        let spec_summary_len = spec.summary.len();
        let spec_index_len = spec.index.len();
        let spec_aref_len = spec.arefs.len();
        let chrom_summary_len = chrom.summary.len();
        let chrom_index_len = chrom.index.len();
        let chrom_aref_len = chrom.arefs.len();
        let spec_meta_len = spec.grouped.byte_len;
        let chrom_meta_len = chrom.grouped.byte_len;

        let off_spec_summary = write_chunk(self.output, spec.summary)?.0;
        let off_spec_entries = write_chunk(self.output, spec.index)?.0;
        let off_spec_arrayrefs = write_chunk(self.output, spec.arefs)?.0;
        let off_chrom_summary = write_chunk(self.output, chrom.summary)?.0;
        let off_chrom_entries = write_chunk(self.output, chrom.index)?.0;
        let off_chrom_arrayrefs = write_chunk(self.output, chrom.arefs)?.0;
        let off_spec_meta = write_chunk(self.output, spec.grouped.section)?.0;
        let off_chrom_meta = write_chunk(self.output, chrom.grouped.section)?.0;
        let off_global_meta = write_aligned(self.output, &global_bytes)?;

        let a3 = self.write_segment_bounds(spec.segment_bounds)?;
        let b3 = self.write_segment_bounds(chrom.segment_bounds)?;

        self.output.write_bytes(&FILE_TRAILER)?;
        let total_file_size = self.output.current_byte_position()?;

        let header = FileHeader {
            compression_codec: self.config.codec_id(),
            compression_level: self.config.compression_level,
            array_filter_id: self.config.array_filter_id(),
            target_block_size: self.config.uncompressed_block_size as u64,

            offset_spec_entries: off_spec_entries,
            len_spec_entries: spec_index_len,
            offset_spec_arrayrefs: off_spec_arrayrefs,
            len_spec_arrayrefs: spec_aref_len,
            offset_chrom_entries: off_chrom_entries,
            len_chrom_entries: chrom_index_len,
            offset_chrom_arrayrefs: off_chrom_arrayrefs,
            len_chrom_arrayrefs: chrom_aref_len,
            offset_spec_meta: off_spec_meta,
            len_spec_meta: spec_meta_len,
            offset_chrom_meta: off_chrom_meta,
            len_chrom_meta: chrom_meta_len,
            offset_global_meta: off_global_meta,
            len_global_meta: global_bytes.len() as u64,
            offset_packed_spectra: spec.container_offset,
            len_packed_spectra: spec.container_total,
            offset_packed_chroms: chrom.container_offset,
            len_packed_chroms: chrom.container_total,

            spectrum_block_count: spec.block_count,
            chrom_block_count: chrom.block_count,
            spectrum_count: spec.count as u64,
            chrom_count: chrom.count as u64,

            spec_meta_row_count: spec.grouped.row_count,
            spec_meta_numeric_count: spec.grouped.numeric_count,
            spec_meta_string_count: spec.grouped.string_count,
            chrom_meta_row_count: chrom.grouped.row_count,
            chrom_meta_numeric_count: chrom.grouped.numeric_count,
            chrom_meta_string_count: chrom.grouped.string_count,
            global_meta_row_count: global_meta.ref_codes.len() as u64,
            global_meta_numeric_count: global_meta.numeric_values.len() as u64,
            global_meta_string_count: global_meta.string_offsets.len() as u64,
            spec_array_type_count: spec.type_count as u64,
            chrom_array_type_count: chrom.type_count as u64,

            spec_meta_uncompressed_size: spec.grouped.uncompressed_size,
            chrom_meta_uncompressed_size: chrom.grouped.uncompressed_size,
            global_meta_uncompressed_size: global_uncompressed,

            meta_group_size: METADATA_GROUP_SIZE,
            spec_meta_group_count: spec.grouped.group_count,
            chrom_meta_group_count: chrom.grouped.group_count,

            off_spec_summary,
            len_spec_summary: spec_summary_len,
            off_chrom_summary,
            len_chrom_summary: chrom_summary_len,

            total_file_size,

            spec_directory_crc32: spec.directory_crc32,
            chrom_directory_crc32: chrom.directory_crc32,

            off_spec_segment_bounds: a3.offset,
            len_spec_segment_bounds: a3.length,
            off_chrom_segment_bounds: b3.offset,
            len_chrom_segment_bounds: b3.length,
            plain_len_spec_segment_bounds: a3.plain_len,
            plain_len_chrom_segment_bounds: b3.plain_len,
            spec_segment_bounds_crc32: a3.crc32,
            chrom_segment_bounds_crc32: b3.crc32,

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

pub fn stream_to_ion(reader: &mut dyn FileReader, writer: &mut IonWriter<'_>) -> IonResult<()> {
    writer.write_reader(reader)
}

pub fn write_mzml_to_ion(
    mzml: &MzML,
    config: EncodingConfig,
    output: &mut dyn EncoderOutput,
) -> IonResult<()> {
    allow_compression_level(config.compression_level)?;
    IonWriter::begin(output, config)?.write_mzml(mzml)
}
