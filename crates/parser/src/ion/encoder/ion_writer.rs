use std::borrow::Borrow;

pub(crate) struct SectionPlacement {
    pub(crate) offset: u64,
    pub(crate) length: u64,
    pub(crate) plain_len: u64,
    pub(crate) crc32: u32,
}

use crate::{
    accessions::{INTENSITY_ARRAY, MZ_ARRAY, TIME_ARRAY},
    ion::{
        IonResult,
        encoder::{
            encode::{
                CHROM_SUMMARY_SIZE, EncodedArrayAddress, SPEC_SUMMARY_SIZE, WriteOptions,
                allow_compression_level, check_spectrum_mz_order, check_spectrum_rt_order,
                encode_single_array, extract_chrom_summary, spec_summary_from_spectrum,
                window_ranges_for_item, write_array_windows,
            },
            scan_stream::ScanStream,
            utilities::{
                BlockWriter, DefaultCompressor, SectionChunk, make_chunk,
                meta_collector::{
                    ArrayPolicy, GroupedSection, LOCAL_LIST_NODE_ID, MetaCollector, MetaGrouper,
                    MzmlListItem, array_type_accession_from_binary_data_array,
                    compress_bytes_if_enabled, serialize_global_meta_with_counts,
                },
                output::WriteBytes,
                tables::{
                    ArrayAddressTable, IndexTable, SummaryTable, WindowDirectory, WindowEntry,
                    write_aligned,
                },
            },
        },
        format::{FILE_TRAILER, HEADER_SIZE},
        header::Header,
        meta_groups::METADATA_GROUP_SIZE,
        utilities::EmitAttributes,
        windowing::WindowRange,
    },
    mzml::structs::{BinaryDataArray, BinaryDataArrayList, Chromatogram, MzML, Spectrum},
};

fn spec_summary_bytes(spec: &Spectrum) -> [u8; SPEC_SUMMARY_SIZE] {
    let s = spec_summary_from_spectrum(spec);
    let mut buf = [0u8; SPEC_SUMMARY_SIZE];
    buf[0..8].copy_from_slice(&s.rt.to_le_bytes());
    buf[8..16].copy_from_slice(&s.base_peak_mz.to_le_bytes());
    buf[16..24].copy_from_slice(&s.selected_ion_mz.to_le_bytes());
    buf[24..32].copy_from_slice(&s.base_peak_int.to_le_bytes());
    buf[32..40].copy_from_slice(&s.total_ion_current.to_le_bytes());
    buf[40] = s.ms_level;
    buf[41] = s.polarity;
    buf[42..46].copy_from_slice(&s.position_x.to_le_bytes());
    buf[46..50].copy_from_slice(&s.position_y.to_le_bytes());
    buf[50..54].copy_from_slice(&s.position_z.to_le_bytes());
    buf[54] = s.rt_unit;
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

struct ArrayWriteState<'a> {
    addresses: &'a mut ArrayAddressTable,
    cursor: &'a mut u64,
    seen: &'a mut Vec<u32>,
}

impl ArrayWriteState<'_> {
    fn emit(&mut self, address: &EncodedArrayAddress) -> IonResult<()> {
        if address.accession != 0 && !self.seen.contains(&address.accession) {
            self.seen.push(address.accession);
        }
        self.addresses.push(
            address.element_offset,
            address.element_count,
            address.block_id,
            address.accession,
            address.dtype,
            address.array_filter,
            address.encoded_len,
            address.continues_previous_segment,
            address.array_cv_code,
        )?;
        *self.cursor += 1;
        Ok(())
    }
}

fn write_single_array(
    bda: &BinaryDataArray,
    config: WriteOptions,
    policy: ArrayPolicy,
    container: &mut BlockWriter<'_, DefaultCompressor>,
    state: &mut ArrayWriteState<'_>,
) -> IonResult<()> {
    let Some(address) = encode_single_array(bda, config, policy, container)? else {
        return Ok(());
    };
    state.emit(&address)?;
    Ok(())
}

fn write_windowed_array(
    bda: &BinaryDataArray,
    config: WriteOptions,
    policy: ArrayPolicy,
    windows: &[WindowRange],
    container: &mut BlockWriter<'_, DefaultCompressor>,
    state: &mut ArrayWriteState<'_>,
) -> IonResult<()> {
    let addresses = write_array_windows(bda, config, policy, windows, container)?;
    for address in &addresses {
        state.emit(address)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_arrays_for<T>(
    item: &T,
    spectrum_index: u32,
    config: WriteOptions,
    policy: ArrayPolicy,
    windowable: bool,
    container: &mut BlockWriter<'_, DefaultCompressor>,
    index: &mut IndexTable,
    state: &mut ArrayWriteState<'_>,
    window_directory: &mut WindowDirectory,
) -> IonResult<()>
where
    T: HasArrayList,
{
    let address_start = *state.cursor;

    if let Some(list) = item.array_list() {
        let windows = if windowable {
            window_ranges_for_item(&list.binary_data_arrays, config, policy, config.mz_window)?
        } else {
            None
        };

        let mut mz_address_start: Option<u64> = None;
        let mut intensity_address_start: Option<u64> = None;

        for bda in &list.binary_data_arrays {
            let accession = array_type_accession_from_binary_data_array(bda);
            let address_start = *state.cursor;
            match windows.as_ref() {
                Some(windows) => {
                    write_windowed_array(bda, config, policy, windows, container, state)?
                }
                None => write_single_array(bda, config, policy, container, state)?,
            }
            if *state.cursor > address_start {
                if accession == policy.x_array_accession {
                    mz_address_start = Some(address_start);
                } else if accession == INTENSITY_ARRAY {
                    intensity_address_start = Some(address_start);
                }
            }
        }

        if let (Some(mz_address_start), Some(intensity_address_start)) =
            (mz_address_start, intensity_address_start)
        {
            push_window_entries(
                window_directory,
                spectrum_index,
                mz_address_start,
                intensity_address_start,
                windows.as_deref(),
            );
        }
    }

    let address_count = *state.cursor - address_start;
    index.push(address_start, address_count)?;
    Ok(())
}

fn push_window_entries(
    window_directory: &mut WindowDirectory,
    spectrum_index: u32,
    mz_address_start: u64,
    intensity_address_start: u64,
    windows: Option<&[WindowRange]>,
) {
    match windows {
        Some(windows) => {
            for (offset, window) in windows.iter().enumerate() {
                window_directory.push(
                    window.window_index,
                    WindowEntry {
                        spectrum_index,
                        mz_address: (mz_address_start + offset as u64) as u32,
                        intensity_address: (intensity_address_start + offset as u64) as u32,
                    },
                );
            }
        }
        None => window_directory.push(
            0,
            WindowEntry {
                spectrum_index,
                mz_address: mz_address_start as u32,
                intensity_address: intensity_address_start as u32,
            },
        ),
    }
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
    addresses: ArrayAddressTable,
    grouper: MetaGrouper,
    window_directory: WindowDirectory,
    windowable: bool,
    count: usize,
    address_cursor: u64,
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
    addresses: SectionChunk,
    window_directory: SectionChunk,
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
        config: WriteOptions,
        windowable: bool,
    ) -> IonResult<Self> {
        let mode = config.section_storage;
        Ok(Self {
            summary: SummaryTable::new(make_chunk(
                mode,
                &format!("{tag}-summary"),
                summary_hint * summary_size,
            )?),
            index: IndexTable::new(make_chunk(mode, &format!("{tag}-index"), table_hint * 16)?),
            addresses: ArrayAddressTable::new(make_chunk(
                mode,
                &format!("{tag}-array_addresses"),
                table_hint * 64,
            )?),
            grouper: MetaGrouper::new(
                METADATA_GROUP_SIZE,
                config.compression_level,
                make_chunk(mode, &format!("{tag}-meta"), 0)?,
            ),
            window_directory: WindowDirectory::new(),
            windowable,
            count: 0,
            address_cursor: 0,
            seen: Vec::with_capacity(8),
            container_offset: 0,
            block_count: 0,
            container_total: 0,
            directory_crc32: 0,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn add<T, L>(
        &mut self,
        item: &T,
        config: WriteOptions,
        policy: ArrayPolicy,
        list_id: u32,
        list_schema: Option<&L>,
        collector: &mut MetaCollector,
        container: &mut BlockWriter<'_, DefaultCompressor>,
        summary: &[u8],
    ) -> IonResult<()>
    where
        T: HasArrayList + MzmlListItem,
        L: EmitAttributes,
    {
        let mut state = ArrayWriteState {
            addresses: &mut self.addresses,
            cursor: &mut self.address_cursor,
            seen: &mut self.seen,
        };
        encode_arrays_for(
            item,
            self.count as u32,
            config,
            policy,
            self.windowable,
            container,
            &mut self.index,
            &mut state,
            &mut self.window_directory,
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
            addresses: self.addresses.finish(),
            window_directory: self.window_directory.finish()?,
            count: self.count,
            type_count: self.seen.len(),
            container_offset: self.container_offset,
            block_count: self.block_count,
            container_total: self.container_total,
            directory_crc32: self.directory_crc32,
        })
    }
}

fn write_chunk(output: &mut dyn WriteBytes, section: SectionChunk) -> IonResult<(u64, u64, u32)> {
    let bytes = section.into_vec()?;
    let crc32 = crc32fast::hash(&bytes);
    let offset = write_aligned(output, &bytes)?;
    Ok((offset, bytes.len() as u64, crc32))
}

#[allow(clippy::too_many_arguments)]
fn write_list<T, L, B, I>(
    output: &mut dyn WriteBytes,
    config: WriteOptions,
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
    let builder = BlockWriter::new(
        output,
        config.block_size,
        compressor,
        config.block_packing_id(),
    );
    let mut container = if config.parallel {
        builder
    } else {
        builder.force_sequential()
    };

    let mut last_rt = f64::NEG_INFINITY;
    let is_spectrum_stream = policy.x_array_accession == MZ_ARRAY;
    for item in items {
        let item = item?;
        let item = item.borrow();
        if is_spectrum_stream && let Some(list) = item.array_list() {
            check_spectrum_mz_order(&list.binary_data_arrays, stream.count)?;
        }
        let summary = summary_of(item);
        if is_spectrum_stream {
            check_spectrum_rt_order(&summary, stream.count, &mut last_rt)?;
        }
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
    output: &'out mut dyn WriteBytes,
    config: WriteOptions,
    collector: MetaCollector,
    spec_list_id: u32,
    chrom_list_id: u32,
    spec_stream: ItemStream,
    chrom_stream: ItemStream,
}

impl<'out> IonWriter<'out> {
    pub fn create(output: &'out mut dyn WriteBytes, config: WriteOptions) -> IonResult<Self> {
        allow_compression_level(config.compression_level)?;
        output.write(&[0u8; HEADER_SIZE])?;

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

    fn write_reader(&mut self, scans: &mut dyn ScanStream) -> IonResult<()> {
        let metadata = scans.metadata()?;
        write_list(
            self.output,
            self.config,
            self.config.array_policy(MZ_ARRAY),
            self.spec_list_id,
            metadata.run.spectrum_list.as_ref(),
            &mut self.collector,
            &mut self.spec_stream,
            std::iter::from_fn(|| scans.next_spectrum().transpose()),
            spec_summary_bytes,
        )?;

        let metadata = scans.metadata()?;
        write_list(
            self.output,
            self.config,
            self.config.array_policy(TIME_ARRAY),
            self.chrom_list_id,
            metadata.run.chromatogram_list.as_ref(),
            &mut self.collector,
            &mut self.chrom_stream,
            std::iter::from_fn(|| scans.next_chromatogram().transpose()),
            chrom_summary_bytes,
        )?;

        let metadata = scans.metadata()?;
        self.finish_inner(&metadata)
    }

    pub fn write_stream(&mut self, scans: &mut dyn ScanStream) -> IonResult<()> {
        self.write_reader(scans)
    }

    fn write_window_directory(&mut self, bounds: SectionChunk) -> IonResult<SectionPlacement> {
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
        let raw_global = serialize_global_meta_with_counts(&global_counts, &global_meta)?;
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
        let spec_address_len = spec.addresses.len();
        let chrom_summary_len = chrom.summary.len();
        let chrom_index_len = chrom.index.len();
        let chrom_address_len = chrom.addresses.len();
        let spec_meta_len = spec.grouped.byte_len;
        let chrom_meta_len = chrom.grouped.byte_len;

        let (off_spec_summary, _, spec_summary_crc32) = write_chunk(self.output, spec.summary)?;
        let (off_spec_entries, _, spec_entries_crc32) = write_chunk(self.output, spec.index)?;
        let (off_spec_array_addresses, _, spec_array_addresses_crc32) =
            write_chunk(self.output, spec.addresses)?;
        let (off_chrom_summary, _, chrom_summary_crc32) = write_chunk(self.output, chrom.summary)?;
        let (off_chrom_entries, _, chrom_entries_crc32) = write_chunk(self.output, chrom.index)?;
        let (off_chrom_array_addresses, _, chrom_array_addresses_crc32) =
            write_chunk(self.output, chrom.addresses)?;
        let off_spec_meta = write_chunk(self.output, spec.grouped.section)?.0;
        let off_chrom_meta = write_chunk(self.output, chrom.grouped.section)?.0;
        let off_global_meta = write_aligned(self.output, &global_bytes)?;

        let a1 = self.write_window_directory(spec.window_directory)?;
        let b1 = self.write_window_directory(chrom.window_directory)?;

        self.output.write(&FILE_TRAILER)?;
        let total_file_size = self.output.position()?;

        let header = Header {
            compression_codec: self.config.codec_id(),
            compression_level: self.config.compression_level,
            default_array_filter: self.config.array_filter_id(),
            target_block_uncompressed_bytes: self.config.block_size as u64,

            off_spec_entries,
            len_spec_entries: spec_index_len,
            off_spec_array_addresses,
            len_spec_array_addresses: spec_address_len,
            off_chrom_entries,
            len_chrom_entries: chrom_index_len,
            off_chrom_array_addresses,
            len_chrom_array_addresses: chrom_address_len,
            off_spec_meta,
            len_spec_meta: spec_meta_len,
            off_chrom_meta,
            len_chrom_meta: chrom_meta_len,
            off_global_meta,
            len_global_meta: global_bytes.len() as u64,
            off_spec_container: spec.container_offset,
            len_spec_container: spec.container_total,
            off_chrom_container: chrom.container_offset,
            len_chrom_container: chrom.container_total,

            spec_block_count: spec.block_count,
            chrom_block_count: chrom.block_count,
            spectrum_count: spec.count as u64,
            chrom_count: chrom.count as u64,

            spec_meta_count: spec.grouped.row_count,
            spec_meta_numeric_count: spec.grouped.numeric_count,
            spec_meta_string_count: spec.grouped.string_count,
            chrom_meta_count: chrom.grouped.row_count,
            chrom_meta_numeric_count: chrom.grouped.numeric_count,
            chrom_meta_string_count: chrom.grouped.string_count,
            global_meta_count: global_meta.ref_codes.len() as u64,
            global_meta_numeric_count: global_meta.numeric_values.len() as u64,
            global_meta_string_count: global_meta.string_offsets.len() as u64,
            spec_array_type_count: spec.type_count as u64,
            chrom_array_type_count: chrom.type_count as u64,

            spec_meta_uncompressed_bytes: spec.grouped.uncompressed_size,
            chrom_meta_uncompressed_bytes: chrom.grouped.uncompressed_size,
            global_meta_uncompressed_bytes: global_uncompressed,

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

            off_spec_window_directory: a1.offset,
            len_spec_window_directory: a1.length,
            off_chrom_window_directory: b1.offset,
            len_chrom_window_directory: b1.length,
            plain_len_spec_window_directory: a1.plain_len,
            plain_len_chrom_window_directory: b1.plain_len,
            spec_window_directory_crc32: a1.crc32,
            chrom_window_directory_crc32: b1.crc32,

            spec_summary_crc32,
            spec_entries_crc32,
            spec_array_addresses_crc32,
            chrom_summary_crc32,
            chrom_entries_crc32,
            chrom_array_addresses_crc32,
            spec_meta_crc32,
            chrom_meta_crc32,
            global_meta_crc32,
            target_mz_window: self.config.mz_window.round() as u32,
            header_crc32: 0,
            ..Header::default()
        };

        let mut header_bytes = [0u8; HEADER_SIZE];
        header.write(&mut header_bytes);
        let crc = crc32fast::hash(&header_bytes[0..1020]);
        header_bytes[1020..1024].copy_from_slice(&crc.to_le_bytes());
        self.output.patch(0, &header_bytes)
    }
}

pub fn write_mzml_to_ion(
    mzml: &MzML,
    config: WriteOptions,
    output: &mut dyn WriteBytes,
) -> IonResult<()> {
    allow_compression_level(config.compression_level)?;
    IonWriter::create(output, config)?.write_mzml(mzml)
}
