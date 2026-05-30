use crate::ion::encoder::utilities::container_builder::{
    BLOCK_DIRECTORY_ENTRY_SIZE, BlockDirEntry, Stride,
};
use crate::ion::packing::PackingId;
use crate::ion::{
    IonError, IonResult,
    utilities::common::{decompress_zstd, read_u32_le_at, read_u64_le_at, take},
};
use crate::ion::utilities::decompression_budget::DecompressionBudget;
use std::ops::Deref;

const LRU_NONE: usize = usize::MAX;

pub(crate) trait BlockProcessor {
    fn decompress(
        &self,
        source: &[u8],
        target_len: usize,
        budget: DecompressionBudget,
    ) -> IonResult<Vec<u8>>;
    fn unshuffle(&self, source: &[u8], target: &mut [u8], stride: usize);
    fn requires_unshuffle(&self, block_packing_id: PackingId) -> bool;
}

pub(crate) trait ContainerAccess {
    fn get_item_from_block(
        &mut self,
        block_id: u32,
        element_offset: u64,
        element_count: u64,
        element_stride: usize,
        ctx: &'static str,
    ) -> IonResult<&[u8]>;
}

#[derive(Debug)]
pub(crate) struct DefaultProcessor;

impl BlockProcessor for DefaultProcessor {
    #[inline]
    fn decompress(
        &self,
        source: &[u8],
        target_len: usize,
        budget: DecompressionBudget,
    ) -> IonResult<Vec<u8>> {
        decompress_zstd(source, target_len, budget)
    }

    #[inline]
    fn unshuffle(&self, source: &[u8], target: &mut [u8], stride: usize) {
        unshuffle_bytes(source, target, stride);
    }

    #[inline]
    fn requires_unshuffle(&self, block_packing_id: PackingId) -> bool {
        block_packing_id == PackingId::ByteShuffle
    }
}

#[derive(Debug)]
pub(crate) enum BlockData<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}

impl<'a> BlockData<'a> {
    #[inline]
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
    cache: Box<[Option<BlockData<'a>>]>,
    lru_prev: Box<[usize]>,
    lru_next: Box<[usize]>,
    lru_head: usize,
    lru_tail: usize,
    cached_bytes: usize,
    max_cached_bytes: usize,
    stride_history: Box<[Option<Stride>]>,
    compression_level: u8,
    block_packing_id: PackingId,
    verify_checksums: bool,
    decompression_budget: DecompressionBudget,
    processor: P,
}

impl<'a, P: BlockProcessor> ContainerView<'a, P> {
    #[allow(dead_code)]
    pub(crate) fn new(
        raw_data: &'a [u8],
        block_count: u64,
        compression_level: u8,
        block_packing_id: PackingId,
        verify_checksums: bool,
        ctx: &'static str,
        processor: P,
        max_cached_bytes: usize,
    ) -> IonResult<Self> {
        Self::with_max_cached_bytes(
            raw_data,
            block_count,
            compression_level,
            block_packing_id,
            verify_checksums,
            ctx,
            processor,
            max_cached_bytes,
            DecompressionBudget::default(),
        )
    }

    pub(crate) fn with_max_cached_bytes(
        raw_data: &'a [u8],
        block_count: u64,
        compression_level: u8,
        block_packing_id: PackingId,
        verify_checksums: bool,
        ctx: &'static str,
        processor: P,
        max_cached_bytes: usize,
        decompression_budget: DecompressionBudget,
    ) -> IonResult<Self> {
        let block_count = validate_block_count(block_count, raw_data.len(), ctx)?;
        let directory_byte_size = block_count * BLOCK_DIRECTORY_ENTRY_SIZE;
        let directory_start_offset = raw_data.len() - directory_byte_size;
        let directory_bytes = &raw_data[directory_start_offset..];
        let mut read_position = 0;
        let mut entries = Vec::with_capacity(block_count);

        for _ in 0..block_count {
            let payload_offset = read_u64_le_at(directory_bytes, &mut read_position, ctx)?;
            let payload_size = read_u64_le_at(directory_bytes, &mut read_position, ctx)?;
            let uncompressed_len_bytes = read_u64_le_at(directory_bytes, &mut read_position, ctx)?;
            let checksum = read_u32_le_at(directory_bytes, &mut read_position, ctx)?;
            let _ = take(directory_bytes, &mut read_position, 4, ctx)?;
            entries.push(BlockDirEntry {
                payload_offset,
                payload_size,
                uncompressed_len_bytes,
                checksum,
            });
        }

        Ok(Self {
            raw_data,
            entries,
            cache: std::iter::repeat_with(|| None)
                .take(block_count)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            lru_prev: vec![LRU_NONE; block_count].into_boxed_slice(),
            lru_next: vec![LRU_NONE; block_count].into_boxed_slice(),
            lru_head: LRU_NONE,
            lru_tail: LRU_NONE,
            cached_bytes: 0,
            max_cached_bytes,
            stride_history: vec![None; block_count].into_boxed_slice(),
            compression_level,
            block_packing_id,
            verify_checksums,
            decompression_budget,
            processor,
        })
    }

    fn block_payload(
        &self,
        block_index: usize,
        ctx: &'static str,
    ) -> IonResult<(BlockDirEntry, &'a [u8])> {
        let entry = *self.entries.get(block_index).ok_or_else(|| {
            IonError::from(format!(
                "{ctx}: block index {block_index} out of range (count={})",
                self.cache.len()
            ))
        })?;
        let payload_start = entry.payload_offset as usize;
        let payload_end = payload_start
            .checked_add(entry.payload_size as usize)
            .ok_or_else(|| {
                IonError::from(format!("{ctx}: block {block_index} payload size overflows"))
            })?;
        let payload_limit = self
            .raw_data
            .len()
            .saturating_sub(self.entries.len() * BLOCK_DIRECTORY_ENTRY_SIZE);

        if payload_end > payload_limit {
            return Err(format!(
                "{ctx}: block {block_index} payload exceeds payload region bounds"
            )
            .into());
        }

        let payload = &self.raw_data[payload_start..payload_end];

        if self.verify_checksums {
            let computed = crc32fast::hash(payload);
            if computed != entry.checksum {
                return Err(format!(
                    "{ctx}: block {block_index} checksum mismatch (stored={:#010x}, computed={:#010x})",
                    entry.checksum, computed
                )
                .into());
            }
        }

        Ok((entry, payload))
    }

    fn decode_owned(
        &self,
        payload: &[u8],
        uncompressed_len: usize,
        stride: Stride,
    ) -> IonResult<Vec<u8>> {
        self.decompression_budget
            .validate(payload.len(), uncompressed_len)?;

        let needs_unshuffle =
            self.processor.requires_unshuffle(self.block_packing_id) && stride != Stride::OneByte;

        let mut data = if self.compression_level == 0 {
            if payload.len() != uncompressed_len {
                return Err(format!(
                    "uncompressed payload size mismatch: got {}, expected {uncompressed_len}",
                    payload.len()
                )
                .into());
            }
            payload.to_vec()
        } else {
            self.processor
                .decompress(payload, uncompressed_len, self.decompression_budget)?
        };

        if needs_unshuffle {
            let mut scratch = vec![0u8; uncompressed_len];
            self.processor
                .unshuffle(&data, &mut scratch, stride.as_usize());
            data = scratch;
        }

        Ok(data)
    }

    pub(crate) fn read_block(
        &self,
        block_id: u32,
        element_stride: usize,
        ctx: &'static str,
    ) -> IonResult<Vec<u8>> {
        let block_index = block_id as usize;
        let stride = Stride::from_size(element_stride);
        let (entry, payload) = self.block_payload(block_index, ctx)?;

        if self.compression_level == 0
            && (!self.processor.requires_unshuffle(self.block_packing_id) || stride == Stride::OneByte)
        {
            if payload.len() != entry.uncompressed_len_bytes as usize {
                return Err(format!(
                    "uncompressed payload size mismatch: got {}, expected {}",
                    payload.len(),
                    entry.uncompressed_len_bytes
                )
                .into());
            }
            return Ok(payload.to_vec());
        }

        self.decode_owned(payload, entry.uncompressed_len_bytes as usize, stride)
    }

    #[inline]
    fn touch_lru(&mut self, block_index: usize) {
        if self.lru_head == block_index {
            return;
        }
        self.detach_lru(block_index);
        self.attach_lru_front(block_index);
    }

    #[inline]
    fn attach_lru_front(&mut self, block_index: usize) {
        self.lru_prev[block_index] = LRU_NONE;
        self.lru_next[block_index] = self.lru_head;
        if self.lru_head != LRU_NONE {
            self.lru_prev[self.lru_head] = block_index;
        } else {
            self.lru_tail = block_index;
        }
        self.lru_head = block_index;
    }

    #[inline]
    fn detach_lru(&mut self, block_index: usize) {
        let prev = self.lru_prev[block_index];
        let next = self.lru_next[block_index];
        if prev != LRU_NONE {
            self.lru_next[prev] = next;
        } else if self.lru_head == block_index {
            self.lru_head = next;
        }
        if next != LRU_NONE {
            self.lru_prev[next] = prev;
        } else if self.lru_tail == block_index {
            self.lru_tail = prev;
        }
        self.lru_prev[block_index] = LRU_NONE;
        self.lru_next[block_index] = LRU_NONE;
    }

    #[inline]
    fn evict_lru_tail(&mut self) {
        let block_index = self.lru_tail;
        if block_index == LRU_NONE {
            return;
        }
        self.detach_lru(block_index);
        if let Some(block) = self.cache[block_index].take() {
            self.cached_bytes = self.cached_bytes.saturating_sub(block.heap_bytes());
        }
    }

    fn evict_until_room(&mut self, needed: usize) {
        if self.max_cached_bytes == 0 {
            return;
        }
        while self
            .cached_bytes
            .checked_add(needed)
            .is_none_or(|total| total > self.max_cached_bytes)
        {
            if self.lru_tail == LRU_NONE {
                break;
            }
            self.evict_lru_tail();
        }
    }

    fn ensure_block_loaded(
        &mut self,
        block_id: u32,
        element_stride: usize,
        ctx: &'static str,
    ) -> IonResult<()> {
        let block_index = block_id as usize;
        if block_index >= self.cache.len() {
            return Err(format!(
                "{ctx}: block index {block_index} out of range (count={})",
                self.cache.len()
            )
            .into());
        }
        if self.cache[block_index].is_some() {
            return Ok(());
        }

        let stride = Stride::from_size(element_stride);
        self.record_stride_or_fail(block_index, stride, ctx)?;
        let (entry, payload) = self.block_payload(block_index, ctx)?;
        let decoded =
            self.run_decode_pipeline(payload, entry.uncompressed_len_bytes as usize, stride)?;
        let block_heap = decoded.heap_bytes();
        self.evict_until_room(block_heap);
        self.cached_bytes += block_heap;
        self.cache[block_index] = Some(decoded);
        self.attach_lru_front(block_index);
        Ok(())
    }

    fn record_stride_or_fail(
        &mut self,
        block_index: usize,
        stride: Stride,
        ctx: &'static str,
    ) -> IonResult<()> {
        if !self.processor.requires_unshuffle(self.block_packing_id) || stride == Stride::OneByte {
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
            )
            .into()),
        }
    }

    fn run_decode_pipeline(
        &mut self,
        payload: &'a [u8],
        uncompressed_len: usize,
        stride: Stride,
    ) -> IonResult<BlockData<'a>> {
        let needs_unshuffle =
            self.processor.requires_unshuffle(self.block_packing_id) && stride != Stride::OneByte;

        if self.compression_level == 0 && !needs_unshuffle {
            if payload.len() != uncompressed_len {
                return Err(format!(
                    "uncompressed payload size mismatch: got {}, expected {uncompressed_len}",
                    payload.len()
                )
                .into());
            }
            return Ok(BlockData::Borrowed(payload));
        }

        Ok(BlockData::Owned(self.decode_owned(
            payload,
            uncompressed_len,
            stride,
        )?))
    }
}

impl<'a, P: BlockProcessor> ContainerAccess for ContainerView<'a, P> {
    #[inline]
    fn get_item_from_block(
        &mut self,
        block_id: u32,
        element_offset: u64,
        element_count: u64,
        element_stride: usize,
        ctx: &'static str,
    ) -> IonResult<&[u8]> {
        self.ensure_block_loaded(block_id, element_stride, ctx)?;

        let block_index = block_id as usize;
        self.touch_lru(block_index);

        let block = self.cache[block_index].as_ref().unwrap();
        let start_byte = usize::try_from(element_offset)
            .ok()
            .and_then(|offset| offset.checked_mul(element_stride))
            .ok_or_else(|| {
                IonError::from(format!("{ctx}: item range overflow for block {block_id}"))
            })?;
        let end_byte = usize::try_from(element_count)
            .ok()
            .and_then(|count| count.checked_mul(element_stride))
            .and_then(|len| start_byte.checked_add(len))
            .ok_or_else(|| {
                IonError::from(format!("{ctx}: item range overflow for block {block_id}"))
            })?;

        if end_byte > block.len() {
            return Err(format!(
                "{ctx}: item range [{start_byte}..{end_byte}] out of bounds for block {block_id} (len={})",
                block.len()
            )
            .into());
        }
        Ok(&block[start_byte..end_byte])
    }
}

#[inline(always)]
fn unshuffle_bytes(source: &[u8], target: &mut [u8], stride: usize) {
    crate::ion::byte_transpose::unshuffle(source, target, stride);
}

#[inline]
fn validate_block_count(
    block_count: u64,
    container_byte_size: usize,
    ctx: &'static str,
) -> IonResult<usize> {
    let max_blocks = (container_byte_size / BLOCK_DIRECTORY_ENTRY_SIZE) as u64;
    if block_count > max_blocks {
        return Err(format!(
            "{ctx}: block_count {block_count} exceeds maximum {max_blocks} derivable from container size {container_byte_size}"
        )
        .into());
    }
    Ok(block_count as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_raw_directory_entry(off: u64, size: u64, uncomp: u64, checksum: u32) -> Vec<u8> {
        let mut e = Vec::with_capacity(BLOCK_DIRECTORY_ENTRY_SIZE);
        e.extend_from_slice(&off.to_le_bytes());
        e.extend_from_slice(&size.to_le_bytes());
        e.extend_from_slice(&uncomp.to_le_bytes());
        e.extend_from_slice(&checksum.to_le_bytes());
        e.extend_from_slice(&[0u8; 4]);
        e
    }

    fn make_container(block_size: usize, count: usize) -> Vec<u8> {
        let mut raw = Vec::new();
        let mut checksums = Vec::with_capacity(count);
        for i in 0..count {
            let payload = vec![i as u8; block_size];
            checksums.push(crc32fast::hash(&payload));
            raw.extend(payload);
        }
        for (i, &checksum) in checksums.iter().enumerate() {
            raw.extend_from_slice(&make_raw_directory_entry(
                (i * block_size) as u64,
                block_size as u64,
                block_size as u64,
                checksum,
            ));
        }
        raw
    }

    #[test]
    fn container_view_rejects_data_smaller_than_directory() {
        let tiny = vec![0u8; 10];
        let result = ContainerView::new(
            &tiny,
            1,
            0,
            PackingId::Raw,
            true,
            "test",
            DefaultProcessor,
            0,
        );
        assert!(result.is_err());
    }

    #[test]
    fn container_view_accepts_empty_container_with_zero_blocks() {
        let empty = vec![];
        assert!(
            ContainerView::new(
                &empty,
                0,
                0,
                PackingId::Raw,
                true,
                "test",
                DefaultProcessor,
                0
            )
            .is_ok()
        );
    }

    #[test]
    fn container_view_get_item_returns_correct_bytes_uncompressed() {
        let payload = vec![0u8, 1, 2, 3, 4, 5, 6, 7];
        let dir = make_raw_directory_entry(0, 8, 8, crc32fast::hash(&payload));
        let mut raw = Vec::new();
        raw.extend_from_slice(&payload);
        raw.extend_from_slice(&dir);

        let mut view = ContainerView::new(
            &raw,
            1,
            0,
            PackingId::Raw,
            true,
            "test",
            DefaultProcessor,
            0,
        )
        .unwrap();
        assert_eq!(
            view.get_item_from_block(0, 1, 1, 4, "test").unwrap(),
            &[4, 5, 6, 7]
        );
    }

    #[test]
    fn container_view_rejects_out_of_bounds_element_range() {
        let payload = vec![0u8; 8];
        let dir = make_raw_directory_entry(0, 8, 8, crc32fast::hash(&payload));
        let mut raw = Vec::new();
        raw.extend_from_slice(&payload);
        raw.extend_from_slice(&dir);

        let mut view = ContainerView::new(
            &raw,
            1,
            0,
            PackingId::Raw,
            true,
            "test",
            DefaultProcessor,
            0,
        )
        .unwrap();
        assert!(view.get_item_from_block(0, 0, 3, 4, "test").is_err());
    }

    #[test]
    fn container_view_rejects_invalid_block_id() {
        let empty = vec![];
        let mut view = ContainerView::new(
            &empty,
            0,
            0,
            PackingId::Raw,
            true,
            "test",
            DefaultProcessor,
            0,
        )
        .unwrap();
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
            PackingId::Raw,
            true,
            "test",
            DefaultProcessor,
            bs * 2,
            DecompressionBudget::default(),
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
            PackingId::Raw,
            true,
            "test",
            DefaultProcessor,
            bs * 2,
            DecompressionBudget::default(),
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
            PackingId::Raw,
            true,
            "test",
            DefaultProcessor,
            0,
            DecompressionBudget::default(),
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
        let dir = make_raw_directory_entry(0, 8, 8, crc32fast::hash(&payload));
        let mut raw = Vec::new();
        raw.extend_from_slice(&payload);
        raw.extend_from_slice(&dir);

        let mut view = ContainerView::with_max_cached_bytes(
            &raw,
            1,
            0,
            PackingId::Raw,
            true,
            "test",
            DefaultProcessor,
            4,
            DecompressionBudget::default(),
        )
        .unwrap();

        view.get_item_from_block(0, 0, 1, 4, "test").unwrap();
        assert_eq!(view.cached_bytes, 8);
    }

    #[test]
    fn checksum_mismatch_is_rejected_when_verification_enabled() {
        let payload = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let wrong_checksum = crc32fast::hash(&payload).wrapping_add(1);
        let dir = make_raw_directory_entry(0, 8, 8, wrong_checksum);
        let mut raw = Vec::new();
        raw.extend_from_slice(&payload);
        raw.extend_from_slice(&dir);

        let mut view = ContainerView::new(
            &raw,
            1,
            0,
            PackingId::Raw,
            true,
            "test",
            DefaultProcessor,
            0,
        )
        .unwrap();
        assert!(view.get_item_from_block(0, 0, 1, 4, "test").is_err());
    }

    #[test]
    fn zero_checksum_is_not_treated_as_bypass() {
        let payload = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let dir = make_raw_directory_entry(0, 8, 8, 0);
        let mut raw = Vec::new();
        raw.extend_from_slice(&payload);
        raw.extend_from_slice(&dir);

        let mut view = ContainerView::new(
            &raw,
            1,
            0,
            PackingId::Raw,
            true,
            "test",
            DefaultProcessor,
            0,
        )
        .unwrap();
        assert!(view.get_item_from_block(0, 0, 1, 4, "test").is_err());
    }

    #[test]
    fn container_view_rejects_block_count_exceeding_directory_capacity() {
        let raw = vec![0u8; 16];
        let result = ContainerView::new(
            &raw,
            10,
            0,
            PackingId::Raw,
            true,
            "test",
            DefaultProcessor,
            0,
        );
        assert!(result.is_err());
    }

    #[test]
    fn unshuffle2_inverts_shuffle2_output() {
        let original = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut shuffled = vec![0u8; original.len()];
        crate::ion::byte_transpose::shuffle(&original, &mut shuffled, 2);
        let mut recovered = vec![0u8; original.len()];
        crate::ion::byte_transpose::unshuffle(&shuffled, &mut recovered, 2);
        assert_eq!(recovered, original);
    }

    #[test]
    fn unshuffle4_inverts_shuffle4_output() {
        let original: Vec<u8> = (0u8..16).collect();
        let mut shuffled = vec![0u8; original.len()];
        crate::ion::byte_transpose::shuffle(&original, &mut shuffled, 4);
        let mut recovered = vec![0u8; original.len()];
        crate::ion::byte_transpose::unshuffle(&shuffled, &mut recovered, 4);
        assert_eq!(recovered, original);
    }
}
