#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use rayon::prelude::*;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use zstd::{bulk::Compressor as ZstdCompressor, zstd_safe::compress_bound};

use crate::encoder::utilities::encoder_output::EncoderOutput;
use crate::ion::byte_transpose::shuffle_with_tail;
use crate::ion::packing::PackingId;
use crate::ion::{IonError, IonResult};

pub(crate) const BLOCK_DIRECTORY_ENTRY_SIZE: usize = 32;
const DEFAULT_MAX_PENDING_BYTES: usize = 64 * 1024 * 1024;

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
    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    fn fork(&self) -> IonResult<Self>
    where
        Self: Sized;
    fn shuffle_bytes_into(&self, input: &[u8], output: &mut [u8], element_stride: usize);
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub(crate) struct DefaultCompressor {
    level: i32,
    inner: ZstdCompressor<'static>,
}

#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
pub(crate) struct DefaultCompressor;

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl DefaultCompressor {
    pub(crate) fn new(compression_level: i32) -> IonResult<Self> {
        Ok(Self {
            level: compression_level,
            inner: ZstdCompressor::new(compression_level)
                .map_err(|err| IonError::from(err.to_string()))?,
        })
    }
}

#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
impl DefaultCompressor {
    pub(crate) fn new(_compression_level: i32) -> IonResult<Self> {
        Err(IonError::from(
            "zstd compression is not available in browser wasm",
        ))
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
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

#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
impl BlockCompressor for DefaultCompressor {
    fn compress(&mut self, _input: &[u8], _output: &mut Vec<u8>) -> IonResult<usize> {
        Err(IonError::from(
            "zstd compression is not available in browser wasm",
        ))
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

fn block_id_from_count(count: usize) -> IonResult<u32> {
    u32::try_from(count)
        .map_err(|_| IonError::from("container: block count exceeds the u32 block id limit"))
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

    fn reserve_next_block_id(&mut self) -> IonResult<u32> {
        let count = self.entries.len();
        let block_id = block_id_from_count(count)?;
        self.entries.push(BlockDirEntry::default());
        Ok(block_id)
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
    compress_sequentially: bool,
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

    fn ensure_open_block(&mut self, stride: Stride, capacity_hint: usize) -> IonResult<()> {
        if !self.slots.is_open(stride) {
            let block_id = self.directory.reserve_next_block_id()?;
            self.slots.insert(
                stride,
                ActiveBlock {
                    block_id,
                    accumulated_data: Vec::with_capacity(capacity_hint),
                    compress_sequentially: false,
                },
            );
        }
        Ok(())
    }

    fn append_to_block<W>(
        &mut self,
        stride: Stride,
        item_byte_size: usize,
        write_action: W,
    ) -> IonResult<(u32, u64)>
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

    fn open_isolated_block(
        &mut self,
        stride: Stride,
        capacity: usize,
        compress_sequentially: bool,
    ) -> IonResult<u32> {
        let block_id = self.directory.reserve_next_block_id()?;
        self.slots.insert(
            stride,
            ActiveBlock {
                block_id,
                accumulated_data: Vec::with_capacity(capacity),
                compress_sequentially,
            },
        );
        Ok(block_id)
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
    compress_sequentially: bool,
}

struct ReadyBlock {
    block_id: u32,
    bytes: Vec<u8>,
    raw_len: u64,
    checksum: u32,
}

fn merge_sorted_by_block_id(left: Vec<ReadyBlock>, right: Vec<ReadyBlock>) -> Vec<ReadyBlock> {
    let mut out = Vec::with_capacity(left.len() + right.len());
    let mut left_iter = left.into_iter();
    let mut right_iter = right.into_iter();
    let mut left_head = left_iter.next();
    let mut right_head = right_iter.next();
    loop {
        match (&left_head, &right_head) {
            (Some(l), Some(r)) => {
                if l.block_id <= r.block_id {
                    out.push(left_head.take().unwrap());
                    left_head = left_iter.next();
                } else {
                    out.push(right_head.take().unwrap());
                    right_head = right_iter.next();
                }
            }
            (Some(_), None) => {
                out.push(left_head.take().unwrap());
                out.extend(left_iter);
                return out;
            }
            (None, Some(_)) => {
                out.push(right_head.take().unwrap());
                out.extend(right_iter);
                return out;
            }
            (None, None) => return out,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContainerSummary {
    pub(crate) block_count: u32,
    pub(crate) total_bytes: u64,
    pub(crate) directory_crc32: u32,
}

pub(crate) struct ContainerBuilder<'output, C: BlockCompressor> {
    output: &'output mut dyn EncoderOutput,
    block_packing_id: PackingId,
    store: BlockStore,
    pending: Vec<PendingBlock>,
    pending_bytes: usize,
    payload_bytes: u64,
    max_pending_bytes: usize,
    compressor: CompressionMode<C>,
    par_min_blocks: usize,
}

impl<'output, C: BlockCompressor + Send> ContainerBuilder<'output, C> {
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
            pending_bytes: 0,
            payload_bytes: 0,
            max_pending_bytes: DEFAULT_MAX_PENDING_BYTES,
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
            self.add_isolated_block(item_byte_size, stride, true, write_action)
        } else {
            self.add_normal_item(item_byte_size, stride, write_action)
        }
    }

    pub(crate) fn add_array_as_block<WriteAction>(
        &mut self,
        item_byte_size: usize,
        element_size: usize,
        write_action: WriteAction,
    ) -> IonResult<(u32, u64)>
    where
        WriteAction: FnOnce(&mut Vec<u8>) -> IonResult<()>,
    {
        let stride = Stride::from_size(element_size.max(1));
        self.add_isolated_block(item_byte_size, stride, false, write_action)
    }

    fn add_isolated_block<WriteAction>(
        &mut self,
        item_byte_size: usize,
        stride: Stride,
        compress_sequentially: bool,
        write_action: WriteAction,
    ) -> IonResult<(u32, u64)>
    where
        WriteAction: FnOnce(&mut Vec<u8>) -> IonResult<()>,
    {
        self.seal_open_block_for_stride(stride)?;

        let block_id =
            self.store
                .open_isolated_block(stride, item_byte_size, compress_sequentially)?;

        write_action(
            &mut self
                .store
                .slots
                .get_mut(stride)
                .expect("isolated block was just inserted")
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

        self.store.ensure_open_block(stride, item_byte_size)?;
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
        let data_len = active_block.accumulated_data.len();
        self.pending.push(PendingBlock {
            block_id: active_block.block_id,
            stride,
            data: active_block.accumulated_data,
            compress_sequentially: active_block.compress_sequentially,
        });
        self.pending_bytes += data_len;
        if self.pending_bytes >= self.max_pending_bytes {
            self.flush_pending()?;
        }
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
                let data = if block_packing_id == PackingId::ByteShuffle
                    && block.stride != Stride::OneByte
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
    fn finish_par(&self, blocks: Vec<PendingBlock>) -> IonResult<Vec<ReadyBlock>> {
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

    #[cfg(test)]
    fn set_max_pending_bytes(&mut self, value: usize) {
        self.max_pending_bytes = value;
    }

    fn flush_pending(&mut self) -> IonResult<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let batch = std::mem::take(&mut self.pending);
        self.pending_bytes = 0;
        let (sequential, shared): (Vec<_>, Vec<_>) =
            batch.into_iter().partition(|b| b.compress_sequentially);

        let sequential_ready = self.finish_seq(sequential)?;

        #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
        let shared_ready = {
            let par_min_blocks = self.par_min_blocks;
            if shared.len() < par_min_blocks {
                self.finish_seq(shared)?
            } else {
                self.finish_par(shared)?
            }
        };
        #[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
        let shared_ready = self.finish_seq(shared)?;

        let ready = merge_sorted_by_block_id(sequential_ready, shared_ready);

        for block in ready {
            self.output.write_bytes(&block.bytes)?;
            self.store.seal(
                block.block_id,
                BlockDirEntry {
                    payload_offset: self.payload_bytes,
                    payload_size: block.bytes.len() as u64,
                    uncompressed_len_bytes: block.raw_len,
                    checksum: block.checksum,
                },
            )?;
            self.payload_bytes += block.bytes.len() as u64;
        }
        Ok(())
    }

    pub(crate) fn finish(mut self) -> IonResult<ContainerSummary> {
        for stride in Stride::all_variants() {
            self.seal_open_block_for_stride(stride)?;
        }
        self.flush_pending()?;

        let block_count = self.store.block_count();
        let mut directory_bytes =
            Vec::with_capacity(block_count as usize * BLOCK_DIRECTORY_ENTRY_SIZE);
        self.store.write_directory(&mut directory_bytes);
        let directory_crc32 = crc32fast::hash(&directory_bytes);
        self.output.write_bytes(&directory_bytes)?;

        let total_bytes = self.payload_bytes + directory_bytes.len() as u64;
        Ok(ContainerSummary {
            block_count,
            total_bytes,
            directory_crc32,
        })
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
        assert_eq!(directory.reserve_next_block_id().unwrap(), 0);
        assert_eq!(directory.reserve_next_block_id().unwrap(), 1);
        assert_eq!(directory.reserve_next_block_id().unwrap(), 2);
        assert_eq!(directory.block_count(), 3);
    }

    #[test]
    fn block_directory_seal_fills_placeholder() {
        let mut directory = BlockDirectory::new();
        let block_id = directory.reserve_next_block_id().unwrap();
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
                compress_sequentially: false,
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
                compress_sequentially: false,
            },
        );
        assert_eq!(slots.byte_len(Stride::FourBytes), 12);
    }

    #[test]
    fn block_store_ensure_open_block_creates_new() {
        let mut store = BlockStore::new(1024);
        assert_eq!(store.block_count(), 0);
        store.ensure_open_block(Stride::FourBytes, 16).unwrap();
        assert_eq!(store.block_count(), 1);
        assert!(store.slots.is_open(Stride::FourBytes));
    }

    #[test]
    fn block_store_ensure_open_block_is_idempotent() {
        let mut store = BlockStore::new(1024);
        store.ensure_open_block(Stride::FourBytes, 16).unwrap();
        store.ensure_open_block(Stride::FourBytes, 16).unwrap();
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
        store.ensure_open_block(Stride::FourBytes, 12).unwrap();
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
        store.ensure_open_block(Stride::EightBytes, 24).unwrap();
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
    fn block_store_open_isolated_block_replaces_slot() {
        let mut store = BlockStore::new(1024);
        store.ensure_open_block(Stride::FourBytes, 8).unwrap();
        let first_id = store.slots.get_mut(Stride::FourBytes).unwrap().block_id;
        let isolated_id = store
            .open_isolated_block(Stride::FourBytes, 256, true)
            .unwrap();
        assert_ne!(first_id, isolated_id);
        assert_eq!(
            store.slots.get_mut(Stride::FourBytes).unwrap().block_id,
            isolated_id
        );
    }

    #[test]
    fn block_store_seal_and_directory_roundtrip() {
        let mut store = BlockStore::new(1024);
        let bid = store.directory.reserve_next_block_id().unwrap();
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
        store.ensure_open_block(Stride::TwoBytes, 8).unwrap();
        store.ensure_open_block(Stride::EightBytes, 8).unwrap();
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
        let ContainerSummary {
            block_count,
            total_bytes,
            ..
        } = builder.finish().unwrap();
        assert_eq!(block_count, 1);
        assert!(total_bytes > 0);
        assert!(output.0.starts_with(&item_data));
    }

    #[test]
    fn add_array_as_block_stays_on_parallel_path() {
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            64 * 1024 * 1024,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        builder
            .add_array_as_block(8, 8, |buf| {
                buf.extend_from_slice(&[1u8; 8]);
                Ok(())
            })
            .unwrap();
        assert_eq!(builder.pending.len(), 1);
        assert!(
            !builder.pending[0].compress_sequentially,
            "split segment blocks must stay on the parallel path"
        );
    }

    #[test]
    fn oversized_item_uses_sequential_path() {
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            16,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        builder
            .add_item_to_box(64, 8, |buf| {
                buf.extend_from_slice(&[2u8; 64]);
                Ok(())
            })
            .unwrap();
        assert_eq!(builder.pending.len(), 1);
        assert!(
            builder.pending[0].compress_sequentially,
            "oversized blocks must stay on the sequential path"
        );
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
        let total_block_count = builder.finish().unwrap().block_count;
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
        let ContainerSummary {
            block_count,
            total_bytes,
            ..
        } = builder.finish().unwrap();
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
        let ContainerSummary {
            block_count,
            total_bytes,
            ..
        } = builder.finish().unwrap();
        assert_eq!(block_count, 0);
        assert_eq!(total_bytes, 0);
        assert!(output.0.is_empty());
    }

    fn ready_block(block_id: u32) -> ReadyBlock {
        ReadyBlock {
            block_id,
            bytes: vec![block_id as u8],
            raw_len: 1,
            checksum: 0,
        }
    }

    #[test]
    fn merge_sorted_by_block_id_interleaves_correctly() {
        let left = vec![ready_block(0), ready_block(3), ready_block(5)];
        let right = vec![ready_block(1), ready_block(2), ready_block(4)];
        let merged = merge_sorted_by_block_id(left, right);
        let ids: Vec<u32> = merged.iter().map(|b| b.block_id).collect();
        assert_eq!(ids, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn merge_sorted_by_block_id_handles_empty_inputs() {
        let only_left = vec![ready_block(0), ready_block(1)];
        let merged = merge_sorted_by_block_id(only_left, Vec::new());
        let ids: Vec<u32> = merged.iter().map(|b| b.block_id).collect();
        assert_eq!(ids, vec![0, 1]);

        let only_right = vec![ready_block(0), ready_block(1)];
        let merged = merge_sorted_by_block_id(Vec::new(), only_right);
        let ids: Vec<u32> = merged.iter().map(|b| b.block_id).collect();
        assert_eq!(ids, vec![0, 1]);

        let merged = merge_sorted_by_block_id(Vec::new(), Vec::new());
        assert!(merged.is_empty());
    }

    #[test]
    fn oversized_item_gets_dedicated_block() {
        let max_block_size = 16usize;
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            max_block_size,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        let (block_id, element_offset) = builder
            .add_item_to_box(64, 8, |buf| {
                buf.extend_from_slice(&[0xABu8; 64]);
                Ok(())
            })
            .unwrap();
        assert_eq!(element_offset, 0);
        let block_count = builder.finish().unwrap().block_count;
        assert_eq!(block_count, 1);
        assert_eq!(block_id, 0);
        assert!(output.0.starts_with(&[0xABu8; 64]));
    }

    #[test]
    fn oversized_item_between_normal_items_produces_three_blocks() {
        let max_block_size = 16usize;
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            max_block_size,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        let (bid_before, _) = builder
            .add_item_to_box(8, 8, |buf| {
                buf.extend_from_slice(&[0x01u8; 8]);
                Ok(())
            })
            .unwrap();
        let (bid_oversized, off_oversized) = builder
            .add_item_to_box(64, 8, |buf| {
                buf.extend_from_slice(&[0x02u8; 64]);
                Ok(())
            })
            .unwrap();
        let (bid_after, _) = builder
            .add_item_to_box(8, 8, |buf| {
                buf.extend_from_slice(&[0x03u8; 8]);
                Ok(())
            })
            .unwrap();
        assert_eq!(off_oversized, 0);
        assert_ne!(bid_before, bid_oversized);
        assert_ne!(bid_oversized, bid_after);
        let block_count = builder.finish().unwrap().block_count;
        assert_eq!(block_count, 3);
    }

    #[test]
    fn two_consecutive_oversized_items_get_separate_dedicated_blocks() {
        let max_block_size = 16usize;
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            max_block_size,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        let (bid_a, off_a) = builder
            .add_item_to_box(64, 8, |buf| {
                buf.extend_from_slice(&[0x11u8; 64]);
                Ok(())
            })
            .unwrap();
        let (bid_b, off_b) = builder
            .add_item_to_box(64, 8, |buf| {
                buf.extend_from_slice(&[0x22u8; 64]);
                Ok(())
            })
            .unwrap();
        assert_eq!(off_a, 0);
        assert_eq!(off_b, 0);
        assert_ne!(bid_a, bid_b);
        let block_count = builder.finish().unwrap().block_count;
        assert_eq!(block_count, 2);
    }

    #[test]
    fn oversized_item_directory_entry_records_actual_uncompressed_size() {
        let max_block_size = 16usize;
        let item_size = 128usize;
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            max_block_size,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        builder
            .add_item_to_box(item_size, 8, |buf| {
                buf.extend_from_slice(&[0xFFu8; 128]);
                Ok(())
            })
            .unwrap();
        let block_count = builder.finish().unwrap().block_count;
        assert_eq!(block_count, 1);
        let directory_start = output.0.len() - BLOCK_DIRECTORY_ENTRY_SIZE;
        let uncomp_bytes = u64::from_le_bytes(
            output.0[directory_start + 16..directory_start + 24]
                .try_into()
                .unwrap(),
        );
        assert_eq!(uncomp_bytes, item_size as u64);
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

    #[test]
    fn batched_flush_keeps_directory_offsets_correct() {
        let item_count = 7usize;
        let mut output = VecOutput(Vec::new());
        let mut builder = ContainerBuilder::new(
            &mut output,
            8,
            CompressionMode::<PassthroughCompressor>::Raw,
            PackingId::Raw,
        );
        builder.set_max_pending_bytes(16);

        for i in 0..item_count {
            builder
                .add_item_to_box(8, 8, |buf| {
                    buf.extend_from_slice(&[i as u8; 8]);
                    Ok(())
                })
                .unwrap();
        }
        let block_count = builder.finish().unwrap().block_count;
        assert_eq!(block_count as usize, item_count);

        let raw = output.0;
        let directory_start = raw.len() - item_count * BLOCK_DIRECTORY_ENTRY_SIZE;
        for i in 0..item_count {
            let at = directory_start + i * BLOCK_DIRECTORY_ENTRY_SIZE;
            let offset = u64::from_le_bytes(raw[at..at + 8].try_into().unwrap()) as usize;
            let size = u64::from_le_bytes(raw[at + 8..at + 16].try_into().unwrap()) as usize;
            assert_eq!(&raw[offset..offset + size], &[i as u8; 8]);
        }
    }

    #[test]
    fn block_id_fits_under_limit() {
        assert_eq!(block_id_from_count(0).unwrap(), 0);
        assert_eq!(block_id_from_count(1).unwrap(), 1);
        assert_eq!(block_id_from_count(u32::MAX as usize).unwrap(), u32::MAX);
    }

    #[test]
    fn block_id_over_limit_errors() {
        assert!(block_id_from_count(u32::MAX as usize + 1).is_err());
    }
}
