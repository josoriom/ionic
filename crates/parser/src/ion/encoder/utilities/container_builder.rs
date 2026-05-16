#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use rayon::prelude::*;
use zstd::{bulk::Compressor as ZstdCompressor, zstd_safe::compress_bound};

use crate::encoder::utilities::encoder_output::EncoderOutput;
use crate::ion::byte_transpose::shuffle_with_tail;
use crate::ion::packing::PackingId;
use crate::ion::{IonError, IonResult};

pub(crate) const BLOCK_DIRECTORY_ENTRY_SIZE: usize = 32;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stride {
    OneByte = 1,
    TwoBytes = 2,
    FourBytes = 4,
    EightBytes = 8,
}

impl Stride {
    #[inline]
    pub(crate) fn from_size(element_size: usize) -> Self {
        match element_size {
            2 => Self::TwoBytes,
            4 => Self::FourBytes,
            8 => Self::EightBytes,
            _ => Self::OneByte,
        }
    }

    #[inline]
    pub(crate) fn as_usize(self) -> usize {
        self as usize
    }

    #[inline]
    fn as_slot_index(self) -> usize {
        match self {
            Self::OneByte => 0,
            Self::TwoBytes => 1,
            Self::FourBytes => 2,
            Self::EightBytes => 3,
        }
    }

    fn all_variants() -> [Stride; 4] {
        [
            Self::OneByte,
            Self::TwoBytes,
            Self::FourBytes,
            Self::EightBytes,
        ]
    }
}

pub(crate) trait BlockCompressor {
    fn compress(&mut self, input: &[u8], output: &mut Vec<u8>) -> IonResult<usize>;
    fn fork(&self) -> IonResult<Self>
    where
        Self: Sized;
    fn shuffle_bytes_into(&self, input: &[u8], output: &mut [u8], element_stride: usize);
}

pub(crate) struct DefaultCompressor {
    level: i32,
    inner: ZstdCompressor<'static>,
}

impl DefaultCompressor {
    pub(crate) fn new(compression_level: i32) -> IonResult<Self> {
        Ok(Self {
            level: compression_level,
            inner: ZstdCompressor::new(compression_level)
                .map_err(|err| IonError::from(err.to_string()))?,
        })
    }
}

impl BlockCompressor for DefaultCompressor {
    fn compress(&mut self, input: &[u8], output: &mut Vec<u8>) -> IonResult<usize> {
        output.clear();
        output.reserve(compress_bound(input.len()));
        self.inner
            .compress_to_buffer(input, output)
            .map_err(|err| IonError::from(err.to_string()))
    }

    fn fork(&self) -> IonResult<Self> {
        Self::new(self.level)
    }

    fn shuffle_bytes_into(&self, input: &[u8], output: &mut [u8], element_stride: usize) {
        shuffle_with_tail(input, output, element_stride);
    }
}

pub(crate) enum CompressionMode<C: BlockCompressor> {
    Raw,
    Compressed(C),
}

#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct BlockDirEntry {
    pub(crate) payload_offset: u64,
    pub(crate) payload_size: u64,
    pub(crate) uncompressed_len_bytes: u64,
    pub(crate) checksum: u32,
}

impl BlockDirEntry {
    fn write_to_buffer(&self, buffer: &mut Vec<u8>) {
        buffer.extend_from_slice(&self.payload_offset.to_le_bytes());
        buffer.extend_from_slice(&self.payload_size.to_le_bytes());
        buffer.extend_from_slice(&self.uncompressed_len_bytes.to_le_bytes());
        buffer.extend_from_slice(&self.checksum.to_le_bytes());
        buffer.extend_from_slice(&[0u8; 4]);
    }
}

struct BlockDirectory {
    entries: Vec<BlockDirEntry>,
}

impl BlockDirectory {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn reserve_next_block_id(&mut self) -> u32 {
        let new_block_id = self.entries.len() as u32;
        self.entries.push(BlockDirEntry::default());
        new_block_id
    }

    fn seal_block(&mut self, block_id: u32, entry: BlockDirEntry) -> IonResult<()> {
        self.entries
            .get_mut(block_id as usize)
            .ok_or_else(|| IonError::from(format!("seal_block: unknown block_id={block_id}")))?
            .clone_from(&entry);
        Ok(())
    }

    fn block_count(&self) -> u32 {
        self.entries.len() as u32
    }

    fn write_to_buffer(&self, buffer: &mut Vec<u8>) {
        for entry in &self.entries {
            entry.write_to_buffer(buffer);
        }
    }
}

struct ActiveBlock {
    block_id: u32,
    accumulated_data: Vec<u8>,
}

struct StrideSlots([Option<ActiveBlock>; 4]);

impl StrideSlots {
    fn new() -> Self {
        Self([None, None, None, None])
    }

    fn get_mut(&mut self, stride: Stride) -> Option<&mut ActiveBlock> {
        self.0[stride.as_slot_index()].as_mut()
    }

    fn insert(&mut self, stride: Stride, block: ActiveBlock) {
        self.0[stride.as_slot_index()] = Some(block);
    }

    fn take(&mut self, stride: Stride) -> Option<ActiveBlock> {
        self.0[stride.as_slot_index()].take()
    }

    fn is_open(&self, stride: Stride) -> bool {
        self.0[stride.as_slot_index()].is_some()
    }

    fn byte_len(&self, stride: Stride) -> usize {
        self.0[stride.as_slot_index()]
            .as_ref()
            .map_or(0, |block| block.accumulated_data.len())
    }
}

struct BlockStore {
    slots: StrideSlots,
    directory: BlockDirectory,
    max_block_size: usize,
}

impl BlockStore {
    fn new(max_block_size: usize) -> Self {
        Self {
            slots: StrideSlots::new(),
            directory: BlockDirectory::new(),
            max_block_size,
        }
    }

    fn would_overflow(&self, stride: Stride, additional_bytes: usize) -> bool {
        let current = self.slots.byte_len(stride);
        self.slots.is_open(stride)
            && current > 0
            && current + additional_bytes > self.max_block_size
    }

    fn ensure_open_block(&mut self, stride: Stride, capacity_hint: usize) {
        if !self.slots.is_open(stride) {
            let block_id = self.directory.reserve_next_block_id();
            self.slots.insert(
                stride,
                ActiveBlock {
                    block_id,
                    accumulated_data: Vec::with_capacity(capacity_hint),
                },
            );
        }
    }

    fn append_to_block<W>(&mut self, stride: Stride, item_byte_size: usize, write_action: W) -> IonResult<(u32, u64)>
    where
        W: FnOnce(&mut Vec<u8>) -> IonResult<()>,
    {
        let active = self
            .slots
            .get_mut(stride)
            .expect("append_to_block: no open block for stride");

        let block_id = active.block_id;
        let element_offset = (active.accumulated_data.len() / stride.as_usize()) as u64;
        active.accumulated_data.reserve(item_byte_size);
        write_action(&mut active.accumulated_data)?;
        Ok((block_id, element_offset))
    }

    fn open_dedicated_block(&mut self, stride: Stride, capacity: usize) -> u32 {
        let block_id = self.directory.reserve_next_block_id();
        self.slots.insert(
            stride,
            ActiveBlock {
                block_id,
                accumulated_data: Vec::with_capacity(capacity),
            },
        );
        block_id
    }

    fn take_open_block(&mut self, stride: Stride) -> Option<ActiveBlock> {
        self.slots.take(stride)
    }

    fn seal(&mut self, block_id: u32, entry: BlockDirEntry) -> IonResult<()> {
        self.directory.seal_block(block_id, entry)
    }

    fn block_count(&self) -> u32 {
        self.directory.block_count()
    }

    fn write_directory(&self, buffer: &mut Vec<u8>) {
        self.directory.write_to_buffer(buffer);
    }
}

struct PendingBlock {
    block_id: u32,
    stride: Stride,
    data: Vec<u8>,
}

struct ReadyBlock {
    block_id: u32,
    bytes: Vec<u8>,
    raw_len: u64,
    checksum: u32,
}

pub(crate) struct ContainerBuilder<'output, C: BlockCompressor> {
    output: &'output mut dyn EncoderOutput,
    block_packing_id: PackingId,
    store: BlockStore,
    pending: Vec<PendingBlock>,
    compressor: CompressionMode<C>,
    par_min_blocks: usize,
}

impl<'output, C: BlockCompressor> ContainerBuilder<'output, C> {
    pub(crate) fn new(
        output: &'output mut dyn EncoderOutput,
        max_block_uncompressed_size: usize,
        compressor: CompressionMode<C>,
        block_packing_id: PackingId,
    ) -> Self {
        Self {
            output,
            block_packing_id,
            store: BlockStore::new(max_block_uncompressed_size),
            pending: Vec::new(),
            compressor,
            par_min_blocks: 4,
        }
    }

    pub(crate) fn force_sequential(mut self) -> Self {
        self.par_min_blocks = usize::MAX;
        self
    }

    pub(crate) fn add_item_to_box<WriteAction>(
        &mut self,
        item_byte_size: usize,
        element_size: usize,
        write_action: WriteAction,
    ) -> IonResult<(u32, u64)>
    where
        WriteAction: FnOnce(&mut Vec<u8>) -> IonResult<()>,
    {
        let stride = Stride::from_size(element_size.max(1));
        if item_byte_size > self.store.max_block_size {
            self.add_oversized_item(item_byte_size, stride, write_action)
        } else {
            self.add_normal_item(item_byte_size, stride, write_action)
        }
    }

    fn add_oversized_item<WriteAction>(
        &mut self,
        item_byte_size: usize,
        stride: Stride,
        write_action: WriteAction,
    ) -> IonResult<(u32, u64)>
    where
        WriteAction: FnOnce(&mut Vec<u8>) -> IonResult<()>,
    {
        self.seal_open_block_for_stride(stride)?;

        let block_id = self.store.open_dedicated_block(stride, item_byte_size);

        write_action(
            &mut self
                .store
                .slots
                .get_mut(stride)
                .expect("dedicated block was just inserted")
                .accumulated_data,
        )?;

        self.seal_open_block_for_stride(stride)?;
        Ok((block_id, 0))
    }

    fn add_normal_item<WriteAction>(
        &mut self,
        item_byte_size: usize,
        stride: Stride,
        write_action: WriteAction,
    ) -> IonResult<(u32, u64)>
    where
        WriteAction: FnOnce(&mut Vec<u8>) -> IonResult<()>,
    {
        if self.store.would_overflow(stride, item_byte_size) {
            self.seal_open_block_for_stride(stride)?;
        }

        self.store.ensure_open_block(stride, item_byte_size);
        let (block_id, element_offset) =
            self.store
                .append_to_block(stride, item_byte_size, write_action)?;

        Ok((block_id, element_offset))
    }

    fn seal_open_block_for_stride(&mut self, stride: Stride) -> IonResult<()> {
        let Some(active_block) = self.store.take_open_block(stride) else {
            return Ok(());
        };
        if active_block.accumulated_data.is_empty() {
            return Ok(());
        }
        self.pending.push(PendingBlock {
            block_id: active_block.block_id,
            stride,
            data: active_block.accumulated_data,
        });
        Ok(())
    }

    fn make_block(
        block_packing_id: PackingId,
        block: PendingBlock,
        mode: &mut CompressionMode<C>,
    ) -> IonResult<ReadyBlock> {
        let raw_len = block.data.len() as u64;
        match mode {
            CompressionMode::Raw => Ok(ReadyBlock {
                block_id: block.block_id,
                checksum: crc32fast::hash(&block.data),
                raw_len,
                bytes: block.data,
            }),
            CompressionMode::Compressed(compressor) => {
                let data =
                    if block_packing_id == PackingId::ByteShuffle && block.stride != Stride::OneByte
                    {
                        let mut shuffled = vec![0u8; block.data.len()];
                        compressor.shuffle_bytes_into(
                            &block.data,
                            &mut shuffled,
                            block.stride.as_usize(),
                        );
                        shuffled
                    } else {
                        block.data
                    };
                let mut bytes = Vec::new();
                compressor.compress(&data, &mut bytes)?;
                Ok(ReadyBlock {
                    block_id: block.block_id,
                    checksum: crc32fast::hash(&bytes),
                    raw_len,
                    bytes,
                })
            }
        }
    }

    fn finish_seq(&mut self, blocks: Vec<PendingBlock>) -> IonResult<Vec<ReadyBlock>> {
        let mut out = Vec::with_capacity(blocks.len());
        for block in blocks {
            out.push(Self::make_block(
                self.block_packing_id,
                block,
                &mut self.compressor,
            )?);
        }
        Ok(out)
    }

    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    fn finish_par(&self, blocks: Vec<PendingBlock>) -> IonResult<Vec<ReadyBlock>>
    where
        C: Send,
    {
        match &self.compressor {
            CompressionMode::Raw => blocks
                .into_par_iter()
                .map(|block| {
                    Ok(ReadyBlock {
                        block_id: block.block_id,
                        checksum: crc32fast::hash(&block.data),
                        raw_len: block.data.len() as u64,
                        bytes: block.data,
                    })
                })
                .collect(),
            CompressionMode::Compressed(compressor) => {
                let mut forks = Vec::with_capacity(blocks.len());
                for _ in 0..blocks.len() {
                    forks.push(compressor.fork()?);
                }
                let block_packing_id = self.block_packing_id;
                blocks
                    .into_par_iter()
                    .zip(forks.into_par_iter())
                    .map(|(block, compressor)| {
                        let mut mode = CompressionMode::Compressed(compressor);
                        Self::make_block(block_packing_id, block, &mut mode)
                    })
                    .collect()
            }
        }
    }

    #[cfg(test)]
    fn set_par_min_blocks(&mut self, value: usize) {
        self.par_min_blocks = value;
    }

    pub(crate) fn finish(mut self) -> IonResult<(u32, u64)>
    where
        C: Send,
    {
        for stride in Stride::all_variants() {
            self.seal_open_block_for_stride(stride)?;
        }

        let blocks = std::mem::take(&mut self.pending);

        #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
        let blocks = {
            let par_min_blocks = self.par_min_blocks;
            if blocks.len() < par_min_blocks {
                self.finish_seq(blocks)?
            } else {
                self.finish_par(blocks)?
            }
        };
        #[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
        let blocks = self.finish_seq(blocks)?;

        let mut payload_bytes = 0u64;
        for block in blocks {
            self.output.write_bytes(&block.bytes)?;
            self.store.seal(
                block.block_id,
                BlockDirEntry {
                    payload_offset: payload_bytes,
                    payload_size: block.bytes.len() as u64,
                    uncompressed_len_bytes: block.raw_len,
                    checksum: block.checksum,
                },
            )?;
            payload_bytes += block.bytes.len() as u64;
        }

        let block_count = self.store.block_count();
        let mut directory_bytes =
            Vec::with_capacity(block_count as usize * BLOCK_DIRECTORY_ENTRY_SIZE);
        self.store.write_directory(&mut directory_bytes);
        self.output.write_bytes(&directory_bytes)?;

        let total_bytes_written = payload_bytes + directory_bytes.len() as u64;
        Ok((block_count, total_bytes_written))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stride_slot_indices_are_unique() {
        let all_indices: Vec<usize> = Stride::all_variants()
            .iter()
            .map(|stride| stride.as_slot_index())
            .collect();
        let mut seen_indices = std::collections::HashSet::new();
        for slot_index in all_indices {
            assert!(
                seen_indices.insert(slot_index),
                "duplicate slot index {slot_index}"
            );
        }
    }

    #[test]
    fn block_directory_allocate_increments() {
        let mut directory = BlockDirectory::new();
        assert_eq!(directory.reserve_next_block_id(), 0);
        assert_eq!(directory.reserve_next_block_id(), 1);
        assert_eq!(directory.reserve_next_block_id(), 2);
        assert_eq!(directory.block_count(), 3);
    }

    #[test]
    fn block_directory_seal_fills_placeholder() {
        let mut directory = BlockDirectory::new();
        let block_id = directory.reserve_next_block_id();
        directory
            .seal_block(
                block_id,
                BlockDirEntry {
                    payload_offset: 10,
                    payload_size: 20,
                    uncompressed_len_bytes: 40,
                    checksum: 0,
                },
            )
            .unwrap();
        assert_eq!(directory.entries[block_id as usize].payload_size, 20);
    }

    #[test]
    fn block_directory_seal_unknown_id_errors() {
        let mut directory = BlockDirectory::new();
        let result = directory.seal_block(99, BlockDirEntry::default());
        assert!(result.is_err());
    }

    #[test]
    fn block_dir_entry_serialises_to_correct_size() {
        let entry = BlockDirEntry {
            payload_offset: 1,
            payload_size: 2,
            uncompressed_len_bytes: 3,
            checksum: 0,
        };
        let mut buffer = Vec::new();
        entry.write_to_buffer(&mut buffer);
        assert_eq!(buffer.len(), BLOCK_DIRECTORY_ENTRY_SIZE);
    }

    #[test]
    fn block_dir_entry_bytes_are_little_endian() {
        let entry = BlockDirEntry {
            payload_offset: 0x0102030405060708,
            payload_size: 0,
            uncompressed_len_bytes: 0,
            checksum: 0,
        };
        let mut buffer = Vec::new();
        entry.write_to_buffer(&mut buffer);
        assert_eq!(
            &buffer[0..8],
            &[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
        );
    }

    #[test]
    fn stride_slots_insert_take_roundtrip() {
        let mut slots = StrideSlots::new();
        assert!(!slots.is_open(Stride::FourBytes));
        slots.insert(
            Stride::FourBytes,
            ActiveBlock {
                block_id: 7,
                accumulated_data: vec![1, 2, 3, 4],
            },
        );
        assert!(slots.is_open(Stride::FourBytes));
        assert!(!slots.is_open(Stride::TwoBytes));
        let taken_block = slots.take(Stride::FourBytes).unwrap();
        assert_eq!(taken_block.block_id, 7);
        assert!(!slots.is_open(Stride::FourBytes));
    }

    #[test]
    fn stride_slots_open_block_byte_len() {
        let mut slots = StrideSlots::new();
        assert_eq!(slots.byte_len(Stride::FourBytes), 0);
        slots.insert(
            Stride::FourBytes,
            ActiveBlock {
                block_id: 0,
                accumulated_data: vec![0u8; 12],
            },
        );
        assert_eq!(slots.byte_len(Stride::FourBytes), 12);
    }

    #[test]
    fn block_store_ensure_open_block_creates_new() {
        let mut store = BlockStore::new(1024);
        assert_eq!(store.block_count(), 0);
        store.ensure_open_block(Stride::FourBytes, 16);
        assert_eq!(store.block_count(), 1);
        assert!(store.slots.is_open(Stride::FourBytes));
    }

    #[test]
    fn block_store_ensure_open_block_is_idempotent() {
        let mut store = BlockStore::new(1024);
        store.ensure_open_block(Stride::FourBytes, 16);
        store.ensure_open_block(Stride::FourBytes, 16);
        assert_eq!(store.block_count(), 1);
    }

    #[test]
    fn block_store_would_overflow_empty_block_returns_false() {
        let store = BlockStore::new(16);
        assert!(!store.would_overflow(Stride::FourBytes, 20));
    }

    #[test]
    fn block_store_would_overflow_detects_threshold() {
        let mut store = BlockStore::new(16);
        store.ensure_open_block(Stride::FourBytes, 12);
        store
            .append_to_block(Stride::FourBytes, 12, |buf| {
                buf.extend_from_slice(&[0u8; 12]);
                Ok(())
            })
            .unwrap();
        assert!(store.would_overflow(Stride::FourBytes, 8));
        assert!(!store.would_overflow(Stride::FourBytes, 4));
    }

    #[test]
    fn block_store_append_returns_correct_element_offsets() {
        let mut store = BlockStore::new(1024);
        store.ensure_open_block(Stride::EightBytes, 24);
        let (_, off0) = store
            .append_to_block(Stride::EightBytes, 8, |b| {
                b.extend_from_slice(&[0u8; 8]);
                Ok(())
            })
            .unwrap();
        let (_, off1) = store
            .append_to_block(Stride::EightBytes, 8, |b| {
                b.extend_from_slice(&[0u8; 8]);
                Ok(())
            })
            .unwrap();
        let (_, off2) = store
            .append_to_block(Stride::EightBytes, 8, |b| {
                b.extend_from_slice(&[0u8; 8]);
                Ok(())
            })
            .unwrap();
        assert_eq!(off0, 0);
        assert_eq!(off1, 1);
        assert_eq!(off2, 2);
    }

    #[test]
    fn block_store_open_dedicated_block_replaces_slot() {
        let mut store = BlockStore::new(1024);
        store.ensure_open_block(Stride::FourBytes, 8);
        let first_id = store.slots.get_mut(Stride::FourBytes).unwrap().block_id;
        let dedicated_id = store.open_dedicated_block(Stride::FourBytes, 256);
        assert_ne!(first_id, dedicated_id);
        assert_eq!(
            store.slots.get_mut(Stride::FourBytes).unwrap().block_id,
            dedicated_id
        );
    }

    #[test]
    fn block_store_seal_and_directory_roundtrip() {
        let mut store = BlockStore::new(1024);
        let bid = store.directory.reserve_next_block_id();
        store
            .seal(
                bid,
                BlockDirEntry {
                    payload_offset: 100,
                    payload_size: 50,
                    uncompressed_len_bytes: 200,
                    checksum: 0,
                },
            )
            .unwrap();
        let mut buf = Vec::new();
        store.write_directory(&mut buf);
        assert_eq!(buf.len(), BLOCK_DIRECTORY_ENTRY_SIZE);
    }

    #[test]
    fn block_store_different_strides_independent() {
        let mut store = BlockStore::new(1024);
        store.ensure_open_block(Stride::TwoBytes, 8);
        store.ensure_open_block(Stride::EightBytes, 8);
        assert_eq!(store.block_count(), 2);
        assert!(store.slots.is_open(Stride::TwoBytes));
        assert!(store.slots.is_open(Stride::EightBytes));
        assert!(!store.slots.is_open(Stride::FourBytes));
    }

    struct VecOutput(Vec<u8>);

    impl EncoderOutput for VecOutput {
        fn write_bytes(&mut self, bytes: &[u8]) -> IonResult<()> {
            self.0.extend_from_slice(bytes);
            Ok(())
        }
        fn patch_bytes_at(&mut self, position: u64, bytes: &[u8]) -> IonResult<()> {
            let start = position as usize;
            self.0[start..start + bytes.len()].copy_from_slice(bytes);
            Ok(())
        }
        fn current_byte_position(&mut self) -> IonResult<u64> {
            Ok(self.0.len() as u64)
        }
    }

    struct PassthroughCompressor;

    impl BlockCompressor for PassthroughCompressor {
        fn compress(&mut self, input: &[u8], output: &mut Vec<u8>) -> IonResult<usize> {
            output.clear();
            output.extend_from_slice(input);
            Ok(input.len())
        }
        fn fork(&self) -> IonResult<Self> {
            Ok(Self)
        }
        fn shuffle_bytes_into(&self, input: &[u8], output: &mut [u8], element_stride: usize) {
            shuffle_with_tail(input, output, element_stride);
        }
    }

    #[test]
    fn container_builder_raw_single_item() {
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            64 * 1024 * 1024,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        let item_data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let (block_id, element_offset) = builder
            .add_item_to_box(item_data.len(), 8, |buf| {
                buf.extend_from_slice(&item_data);
                Ok(())
            })
            .unwrap();
        assert_eq!(block_id, 0);
        assert_eq!(element_offset, 0);
        let (block_count, total_bytes) = builder.finish().unwrap();
        assert_eq!(block_count, 1);
        assert!(total_bytes > 0);
        assert!(output.0.starts_with(&item_data));
    }

    #[test]
    fn container_builder_element_offsets_are_correct() {
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            64 * 1024 * 1024,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        let (_, first_offset) = builder
            .add_item_to_box(8, 8, |buf| {
                buf.extend_from_slice(&[0u8; 8]);
                Ok(())
            })
            .unwrap();
        let (_, second_offset) = builder
            .add_item_to_box(8, 8, |buf| {
                buf.extend_from_slice(&[0u8; 8]);
                Ok(())
            })
            .unwrap();
        let (_, third_offset) = builder
            .add_item_to_box(8, 8, |buf| {
                buf.extend_from_slice(&[0u8; 8]);
                Ok(())
            })
            .unwrap();
        assert_eq!(first_offset, 0);
        assert_eq!(second_offset, 1);
        assert_eq!(third_offset, 2);
    }

    #[test]
    fn container_builder_different_strides_get_different_blocks() {
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            64 * 1024 * 1024,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        let (four_byte_block_id, _) = builder
            .add_item_to_box(4, 4, |buf| {
                buf.extend_from_slice(&[0u8; 4]);
                Ok(())
            })
            .unwrap();
        let (eight_byte_block_id, _) = builder
            .add_item_to_box(8, 8, |buf| {
                buf.extend_from_slice(&[0u8; 8]);
                Ok(())
            })
            .unwrap();
        assert_ne!(four_byte_block_id, eight_byte_block_id);
    }

    #[test]
    fn container_builder_block_splits_when_full() {
        let max_block_size = 16usize;
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            max_block_size,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        let (first_block_id, _) = builder
            .add_item_to_box(12, 4, |buf| {
                buf.extend_from_slice(&[0u8; 12]);
                Ok(())
            })
            .unwrap();
        let (second_block_id, _) = builder
            .add_item_to_box(12, 4, |buf| {
                buf.extend_from_slice(&[0u8; 12]);
                Ok(())
            })
            .unwrap();
        assert_ne!(
            first_block_id, second_block_id,
            "overflow should have triggered a new block"
        );
        let (total_block_count, _) = builder.finish().unwrap();
        assert_eq!(total_block_count, 2);
    }

    #[test]
    fn container_builder_finish_writes_directory() {
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            64 * 1024 * 1024,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        builder
            .add_item_to_box(8, 8, |buf| {
                buf.extend_from_slice(&[0xAAu8; 8]);
                Ok(())
            })
            .unwrap();
        let (block_count, total_bytes) = builder.finish().unwrap();
        assert_eq!(block_count, 1);
        let expected_directory_size = BLOCK_DIRECTORY_ENTRY_SIZE as u64;
        assert_eq!(total_bytes, 8 + expected_directory_size);
    }

    #[test]
    fn container_builder_empty_produces_no_blocks() {
        let mut output = VecOutput(Vec::new());
        let builder = ContainerBuilder::new(
            &mut output,
            64 * 1024 * 1024,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        let (block_count, total_bytes) = builder.finish().unwrap();
        assert_eq!(block_count, 0);
        assert_eq!(total_bytes, 0);
        assert!(output.0.is_empty());
    }

    #[test]
    fn container_builder_seq_and_par_match() {
        let mut seq_out = VecOutput(Vec::new());
        let mut seq = ContainerBuilder::new(
            &mut seq_out,
            8,
            CompressionMode::Compressed(PassthroughCompressor),
            PackingId::ByteShuffle,
        );
        seq.set_par_min_blocks(usize::MAX);

        let mut par_out = VecOutput(Vec::new());
        let mut par = ContainerBuilder::new(
            &mut par_out,
            8,
            CompressionMode::Compressed(PassthroughCompressor),
            PackingId::ByteShuffle,
        );
        par.set_par_min_blocks(0);

        for builder in [&mut seq, &mut par] {
            builder
                .add_item_to_box(8, 4, |buf| {
                    buf.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
                    Ok(())
                })
                .unwrap();
            builder
                .add_item_to_box(8, 4, |buf| {
                    buf.extend_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
                    Ok(())
                })
                .unwrap();
            builder
                .add_item_to_box(8, 4, |buf| {
                    buf.extend_from_slice(&[17, 18, 19, 20, 21, 22, 23, 24]);
                    Ok(())
                })
                .unwrap();
            builder
                .add_item_to_box(8, 4, |buf| {
                    buf.extend_from_slice(&[25, 26, 27, 28, 29, 30, 31, 32]);
                    Ok(())
                })
                .unwrap();
        }

        let seq_res = seq.finish().unwrap();
        let par_res = par.finish().unwrap();

        assert_eq!(seq_res, par_res);
        assert_eq!(seq_out.0, par_out.0);
    }
}
