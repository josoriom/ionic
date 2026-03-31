use crate::ion::encoder::utilities::container_builder::{
    BLOCK_DIRECTORY_ENTRY_SIZE, BlockDirEntry, FilterType, Stride,
};
use crate::ion::utilities::common::{decompress_zstd, read_u32_le_at, read_u64_le_at, take};
use std::ops::Deref;

const DEFAULT_MAX_CACHED_BYTES: usize = 64 * 1024 * 1024;

pub(crate) trait BlockProcessor {
    fn decompress(&self, source: &[u8], target_len: usize) -> Result<Vec<u8>, String>;
    fn unshuffle(&self, source: &[u8], target: &mut [u8], stride: usize);
    fn requires_unshuffle(&self, filter: FilterType) -> bool;
}

#[derive(Debug)]
pub(crate) struct DefaultProcessor;

impl BlockProcessor for DefaultProcessor {
    #[inline]
    fn decompress(&self, source: &[u8], target_len: usize) -> Result<Vec<u8>, String> {
        decompress_zstd(source, target_len)
    }

    #[inline]
    fn unshuffle(&self, source: &[u8], target: &mut [u8], stride: usize) {
        unshuffle_bytes(source, target, stride);
    }

    #[inline]
    fn requires_unshuffle(&self, filter: FilterType) -> bool {
        filter == FilterType::Shuffle
    }
}

#[derive(Debug)]
pub(crate) enum BlockData<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}

impl<'a> BlockData<'a> {
    fn heap_bytes(&self) -> usize {
        self.len()
    }
}

impl<'a> Deref for BlockData<'a> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(data) => data,
            Self::Owned(data) => data.as_slice(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ContainerView<'a, P: BlockProcessor> {
    raw_data: &'a [u8],
    entries: Vec<BlockDirEntry>,
    cache: Vec<Option<BlockData<'a>>>,
    access_tick: Vec<u64>,
    tick_counter: u64,
    cached_bytes: usize,
    max_cached_bytes: usize,
    scratch_buffer: Vec<u8>,
    stride_history: Vec<Option<Stride>>,
    compression_level: u8,
    filter: FilterType,
    processor: P,
}

impl<'a, P: BlockProcessor> ContainerView<'a, P> {
    #[allow(dead_code)]
    pub(crate) fn new(
        raw_data: &'a [u8],
        block_count: u64,
        compression_level: u8,
        filter: FilterType,
        ctx: &'static str,
        processor: P,
    ) -> Result<Self, String> {
        Self::with_max_cached_bytes(
            raw_data,
            block_count,
            compression_level,
            filter,
            ctx,
            processor,
            DEFAULT_MAX_CACHED_BYTES,
        )
    }

    pub(crate) fn with_max_cached_bytes(
        raw_data: &'a [u8],
        block_count: u64,
        compression_level: u8,
        filter: FilterType,
        ctx: &'static str,
        processor: P,
        max_cached_bytes: usize,
    ) -> Result<Self, String> {
        let block_count = block_count as usize;
        let directory_byte_size = block_count * BLOCK_DIRECTORY_ENTRY_SIZE;

        if raw_data.len() < directory_byte_size {
            return Err(format!(
                "{ctx}: container too small to hold block directory"
            ));
        }

        let directory_start_offset = raw_data.len() - directory_byte_size;
        let directory_bytes = &raw_data[directory_start_offset..];
        let mut read_position = 0;
        let mut entries = Vec::with_capacity(block_count);

        for _ in 0..block_count {
            let payload_offset = read_u64_le_at(directory_bytes, &mut read_position, ctx)?;
            let payload_size = read_u64_le_at(directory_bytes, &mut read_position, ctx)?;
            let uncompressed_len_bytes = read_u64_le_at(directory_bytes, &mut read_position, ctx)?;
            let checksum = read_u32_le_at(directory_bytes, &mut read_position, ctx)?;
            let _reserved_padding = take(directory_bytes, &mut read_position, 4, ctx)?;
            entries.push(BlockDirEntry {
                payload_offset,
                payload_size,
                uncompressed_len_bytes,
                checksum,
            });
        }

        let mut cache = Vec::with_capacity(block_count);
        cache.resize_with(block_count, || None);

        Ok(Self {
            raw_data,
            entries,
            cache,
            access_tick: vec![0u64; block_count],
            tick_counter: 0,
            cached_bytes: 0,
            max_cached_bytes,
            scratch_buffer: Vec::new(),
            stride_history: vec![None; block_count],
            compression_level,
            filter,
            processor,
        })
    }

    #[inline]
    pub(crate) fn get_item_from_block(
        &mut self,
        block_id: u32,
        element_offset: u64,
        element_count: u64,
        element_stride: usize,
        ctx: &'static str,
    ) -> Result<&[u8], String> {
        self.ensure_block_loaded(block_id, element_stride, ctx)?;

        let block_index = block_id as usize;
        self.tick_counter += 1;
        self.access_tick[block_index] = self.tick_counter;

        let block = self.cache[block_index].as_ref().unwrap();
        let start_byte = (element_offset as usize) * element_stride;
        let end_byte = start_byte + (element_count as usize) * element_stride;

        if end_byte > block.len() {
            return Err(format!(
                "{ctx}: item range [{start_byte}..{end_byte}] out of bounds for block {block_id} (len={})",
                block.len()
            ));
        }
        Ok(&block[start_byte..end_byte])
    }

    fn evict_until_room(&mut self, needed: usize) {
        if self.max_cached_bytes == 0 {
            return;
        }
        while self.cached_bytes + needed > self.max_cached_bytes {
            let mut oldest_tick = u64::MAX;
            let mut oldest_idx = usize::MAX;

            for (i, slot) in self.cache.iter().enumerate() {
                if slot.is_some() && self.access_tick[i] < oldest_tick {
                    oldest_tick = self.access_tick[i];
                    oldest_idx = i;
                }
            }

            if oldest_idx == usize::MAX {
                break;
            }

            if let Some(block) = self.cache[oldest_idx].take() {
                self.cached_bytes -= block.heap_bytes();
            }
        }
    }

    fn ensure_block_loaded(
        &mut self,
        block_id: u32,
        element_stride: usize,
        ctx: &'static str,
    ) -> Result<(), String> {
        let block_index = block_id as usize;
        if block_index >= self.cache.len() {
            return Err(format!(
                "{ctx}: block index {block_index} out of range (count={})",
                self.cache.len()
            ));
        }
        if self.cache[block_index].is_some() {
            return Ok(());
        }

        let stride = Stride::from_size(element_stride);
        self.record_stride_or_fail(block_index, stride, ctx)?;

        let entry = self.entries[block_index];
        let payload_start = entry.payload_offset as usize;
        let payload_end = payload_start
            .checked_add(entry.payload_size as usize)
            .ok_or_else(|| format!("{ctx}: block {block_index} payload size overflows"))?;

        if payload_end > self.raw_data.len() - (self.entries.len() * BLOCK_DIRECTORY_ENTRY_SIZE) {
            return Err(format!(
                "{ctx}: block {block_index} payload exceeds payload region bounds"
            ));
        }

        let payload = &self.raw_data[payload_start..payload_end];

        if entry.checksum != 0 {
            let computed = crc32fast::hash(payload);
            if computed != entry.checksum {
                return Err(format!(
                    "{ctx}: block {block_index} checksum mismatch (stored={:#010x}, computed={:#010x})",
                    entry.checksum, computed
                ));
            }
        }

        let decoded =
            self.run_decode_pipeline(payload, entry.uncompressed_len_bytes as usize, stride)?;

        let block_heap = decoded.heap_bytes();
        self.evict_until_room(block_heap);

        self.cached_bytes += block_heap;
        self.cache[block_index] = Some(decoded);
        self.tick_counter += 1;
        self.access_tick[block_index] = self.tick_counter;
        Ok(())
    }

    fn record_stride_or_fail(
        &mut self,
        block_index: usize,
        stride: Stride,
        ctx: &'static str,
    ) -> Result<(), String> {
        if !self.processor.requires_unshuffle(self.filter) || stride == Stride::OneByte {
            return Ok(());
        }
        match self.stride_history[block_index] {
            None => {
                self.stride_history[block_index] = Some(stride);
                Ok(())
            }
            Some(recorded) if recorded == stride => Ok(()),
            Some(recorded) => Err(format!(
                "{ctx}: stride mismatch for block {block_index} (expected {recorded:?}, got {stride:?})"
            )),
        }
    }

    fn run_decode_pipeline(
        &mut self,
        payload: &'a [u8],
        uncompressed_len: usize,
        stride: Stride,
    ) -> Result<BlockData<'a>, String> {
        let needs_unshuffle =
            self.processor.requires_unshuffle(self.filter) && stride != Stride::OneByte;

        if self.compression_level == 0 && !needs_unshuffle {
            if payload.len() != uncompressed_len {
                return Err(format!(
                    "uncompressed payload size mismatch: got {}, expected {uncompressed_len}",
                    payload.len()
                ));
            }
            return Ok(BlockData::Borrowed(payload));
        }

        let mut decompressed = if self.compression_level == 0 {
            payload.to_vec()
        } else {
            self.processor.decompress(payload, uncompressed_len)?
        };

        if needs_unshuffle {
            self.scratch_buffer.resize(uncompressed_len, 0);
            self.processor
                .unshuffle(&decompressed, &mut self.scratch_buffer, stride.as_usize());
            std::mem::swap(&mut decompressed, &mut self.scratch_buffer);
        }

        Ok(BlockData::Owned(decompressed))
    }
}

#[inline(always)]
fn unshuffle_bytes(source: &[u8], target: &mut [u8], stride: usize) {
    match stride {
        8 => unshuffle8(source, target),
        4 => unshuffle4(source, target),
        2 => unshuffle2(source, target),
        _ => unshuffle_any(source, target, stride),
    }
}

#[inline(always)]
fn unshuffle2(source: &[u8], target: &mut [u8]) {
    let half = source.len() / 2;
    let (first_half, second_half) = source.split_at(half);
    for i in 0..half {
        target[i * 2] = first_half[i];
        target[i * 2 + 1] = second_half[i];
    }
}

#[inline(always)]
fn unshuffle4(source: &[u8], target: &mut [u8]) {
    let quarter = source.len() / 4;
    let (g0, rest) = source.split_at(quarter);
    let (g1, rest) = rest.split_at(quarter);
    let (g2, g3) = rest.split_at(quarter);
    for i in 0..quarter {
        let o = i * 4;
        target[o] = g0[i];
        target[o + 1] = g1[i];
        target[o + 2] = g2[i];
        target[o + 3] = g3[i];
    }
}

#[inline(always)]
fn unshuffle8(source: &[u8], target: &mut [u8]) {
    let seg = source.len() / 8;
    let (g0, rest) = source.split_at(seg);
    let (g1, rest) = rest.split_at(seg);
    let (g2, rest) = rest.split_at(seg);
    let (g3, rest) = rest.split_at(seg);
    let (g4, rest) = rest.split_at(seg);
    let (g5, rest) = rest.split_at(seg);
    let (g6, g7) = rest.split_at(seg);
    for i in 0..seg {
        let o = i * 8;
        target[o] = g0[i];
        target[o + 1] = g1[i];
        target[o + 2] = g2[i];
        target[o + 3] = g3[i];
        target[o + 4] = g4[i];
        target[o + 5] = g5[i];
        target[o + 6] = g6[i];
        target[o + 7] = g7[i];
    }
}

#[inline(always)]
fn unshuffle_any(source: &[u8], target: &mut [u8], stride: usize) {
    let element_count = source.len() / stride;
    for byte_position in 0..stride {
        let source_base = byte_position * element_count;
        for element_index in 0..element_count {
            target[byte_position + element_index * stride] = source[source_base + element_index];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_raw_directory_entry(off: u64, size: u64, uncomp: u64) -> Vec<u8> {
        let mut e = Vec::with_capacity(BLOCK_DIRECTORY_ENTRY_SIZE);
        e.extend_from_slice(&off.to_le_bytes());
        e.extend_from_slice(&size.to_le_bytes());
        e.extend_from_slice(&uncomp.to_le_bytes());
        e.extend_from_slice(&0u32.to_le_bytes());
        e.extend_from_slice(&[0u8; 4]);
        e
    }

    fn make_container(block_size: usize, count: usize) -> Vec<u8> {
        let mut raw = Vec::new();
        for i in 0..count {
            raw.extend(vec![i as u8; block_size]);
        }
        for i in 0..count {
            raw.extend_from_slice(&make_raw_directory_entry(
                (i * block_size) as u64,
                block_size as u64,
                block_size as u64,
            ));
        }
        raw
    }

    #[test]
    fn container_view_rejects_data_smaller_than_directory() {
        let tiny = vec![0u8; 10];
        let result = ContainerView::new(&tiny, 1, 0, FilterType::None, "test", DefaultProcessor);
        assert!(result.is_err());
    }

    #[test]
    fn container_view_accepts_empty_container_with_zero_blocks() {
        let empty = vec![];
        assert!(
            ContainerView::new(&empty, 0, 0, FilterType::None, "test", DefaultProcessor).is_ok()
        );
    }

    #[test]
    fn container_view_get_item_returns_correct_bytes_uncompressed() {
        let payload = vec![0u8, 1, 2, 3, 4, 5, 6, 7];
        let dir = make_raw_directory_entry(0, 8, 8);
        let mut raw = Vec::new();
        raw.extend_from_slice(&payload);
        raw.extend_from_slice(&dir);

        let mut view =
            ContainerView::new(&raw, 1, 0, FilterType::None, "test", DefaultProcessor).unwrap();
        assert_eq!(
            view.get_item_from_block(0, 1, 1, 4, "test").unwrap(),
            &[4, 5, 6, 7]
        );
    }

    #[test]
    fn container_view_rejects_out_of_bounds_element_range() {
        let payload = vec![0u8; 8];
        let dir = make_raw_directory_entry(0, 8, 8);
        let mut raw = Vec::new();
        raw.extend_from_slice(&payload);
        raw.extend_from_slice(&dir);

        let mut view =
            ContainerView::new(&raw, 1, 0, FilterType::None, "test", DefaultProcessor).unwrap();
        assert!(view.get_item_from_block(0, 0, 3, 4, "test").is_err());
    }

    #[test]
    fn container_view_rejects_invalid_block_id() {
        let empty = vec![];
        let mut view =
            ContainerView::new(&empty, 0, 0, FilterType::None, "test", DefaultProcessor).unwrap();
        assert!(view.get_item_from_block(99, 0, 1, 4, "test").is_err());
    }

    #[test]
    fn lru_evicts_when_byte_limit_exceeded() {
        let bs = 100usize;
        let raw = make_container(bs, 3);

        let mut view = ContainerView::with_max_cached_bytes(
            &raw,
            3,
            0,
            FilterType::None,
            "test",
            DefaultProcessor,
            bs * 2,
        )
        .unwrap();

        view.get_item_from_block(0, 0, 1, bs, "test").unwrap();
        view.get_item_from_block(1, 0, 1, bs, "test").unwrap();
        assert_eq!(view.cached_bytes, bs * 2);

        view.get_item_from_block(2, 0, 1, bs, "test").unwrap();
        assert_eq!(view.cached_bytes, bs * 2);
        assert!(view.cache[0].is_none());
        assert!(view.cache[1].is_some());
        assert!(view.cache[2].is_some());
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        let bs = 100usize;
        let raw = make_container(bs, 3);

        let mut view = ContainerView::with_max_cached_bytes(
            &raw,
            3,
            0,
            FilterType::None,
            "test",
            DefaultProcessor,
            bs * 2,
        )
        .unwrap();

        view.get_item_from_block(0, 0, 1, bs, "test").unwrap();
        view.get_item_from_block(1, 0, 1, bs, "test").unwrap();
        view.get_item_from_block(0, 0, 1, bs, "test").unwrap();
        view.get_item_from_block(2, 0, 1, bs, "test").unwrap();

        assert!(view.cache[0].is_some());
        assert!(view.cache[1].is_none());
        assert!(view.cache[2].is_some());
    }

    #[test]
    fn zero_max_bytes_means_unlimited() {
        let bs = 100usize;
        let raw = make_container(bs, 4);

        let mut view = ContainerView::with_max_cached_bytes(
            &raw,
            4,
            0,
            FilterType::None,
            "test",
            DefaultProcessor,
            0,
        )
        .unwrap();

        for i in 0..4 {
            view.get_item_from_block(i, 0, 1, bs, "test").unwrap();
        }
        assert_eq!(view.cached_bytes, bs * 4);
        assert!(view.cache.iter().all(|s| s.is_some()));
    }

    #[test]
    fn borrowed_blocks_dont_count_toward_byte_limit() {
        let payload = vec![0u8; 8];
        let dir = make_raw_directory_entry(0, 8, 8);
        let mut raw = Vec::new();
        raw.extend_from_slice(&payload);
        raw.extend_from_slice(&dir);

        let mut view = ContainerView::with_max_cached_bytes(
            &raw,
            1,
            0,
            FilterType::None,
            "test",
            DefaultProcessor,
            4,
        )
        .unwrap();

        view.get_item_from_block(0, 0, 1, 4, "test").unwrap();
        assert_eq!(view.cached_bytes, 8);
    }

    #[test]
    fn unshuffle2_inverts_shuffle2_output() {
        let original = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let half = original.len() / 2;
        let mut shuffled = vec![0u8; original.len()];
        for i in 0..half {
            shuffled[i] = original[i * 2];
            shuffled[i + half] = original[i * 2 + 1];
        }
        let mut recovered = vec![0u8; original.len()];
        unshuffle2(&shuffled, &mut recovered);
        assert_eq!(recovered, original);
    }

    #[test]
    fn unshuffle4_inverts_shuffle4_output() {
        let original: Vec<u8> = (0u8..16).collect();
        let quarter = original.len() / 4;
        let mut shuffled = vec![0u8; original.len()];
        for i in 0..quarter {
            let o = i * 4;
            shuffled[i] = original[o];
            shuffled[i + quarter] = original[o + 1];
            shuffled[i + 2 * quarter] = original[o + 2];
            shuffled[i + 3 * quarter] = original[o + 3];
        }
        let mut recovered = vec![0u8; original.len()];
        unshuffle4(&shuffled, &mut recovered);
        assert_eq!(recovered, original);
    }
}
