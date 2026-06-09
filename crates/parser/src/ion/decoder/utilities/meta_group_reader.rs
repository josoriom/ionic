use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::{
    decoder::decode::{Metadatum, MetadatumValue},
    decoder::utilities::{
        common::decompress_zstd_allow_aligned_padding,
        decompression_budget::DecompressionBudget,
        parse_metadata::{CODEC_NONE, CODEC_ZSTD, parse_metadata},
    },
    ion::{
        IonError, IonResult,
        meta_groups::{
            META_GROUP_ENTRY_SIZE, META_GROUP_HEADER_SIZE, MetaGroupEntry, MetaTotals,
            group_count_for, group_of_item, item_range_of_group, read_group_header,
        },
    },
};

pub(crate) struct MetaGroupReader {
    section: Arc<[u8]>,
    directory: Vec<MetaGroupEntry>,
    group_size: u32,
    item_count: u64,
    totals: MetaTotals,
    payload_end: usize,
    compression_codec: u8,
    verify_checksums: bool,
    budget: DecompressionBudget,
    cache: GroupCache,
}

#[derive(Default)]
struct MetaCounts {
    rows: u64,
    numeric: u64,
    string: u64,
}

impl MetaCounts {
    fn add(&mut self, rows: u32, numeric: u32, string: u32) {
        self.rows += rows as u64;
        self.numeric += numeric as u64;
        self.string += string as u64;
    }
}

struct CachedGroup {
    rows: Arc<[Metadatum]>,
    footprint: usize,
}

struct GroupCache {
    groups: HashMap<u64, CachedGroup>,
    order: VecDeque<u64>,
    used_bytes: usize,
    max_bytes: usize,
}

impl MetaGroupReader {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        section: Arc<[u8]>,
        group_count: u64,
        group_size: u32,
        item_count: u64,
        totals: MetaTotals,
        compression_codec: u8,
        verify_checksums: bool,
        budget: DecompressionBudget,
        max_cached_bytes: usize,
    ) -> IonResult<Self> {
        if item_count > 0 && group_size == 0 {
            return Err(IonError::from("metadata groups: group size is zero"));
        }
        if group_count != group_count_for(item_count, group_size) {
            return Err(IonError::from(
                "metadata groups: group count does not match item count",
            ));
        }
        let directory = read_directory(&section, group_count)?;
        let payload_end = section.len() - directory.len() * META_GROUP_ENTRY_SIZE;
        let mut uncompressed_sum: u64 = 0;
        for entry in &directory {
            uncompressed_sum = uncompressed_sum
                .checked_add(entry.uncompressed_size)
                .ok_or_else(|| IonError::from("metadata groups: uncompressed total overflows"))?;
        }
        if uncompressed_sum != totals.uncompressed {
            return Err(IonError::from(
                "metadata groups: uncompressed total does not match header",
            ));
        }
        Ok(Self {
            section,
            directory,
            group_size,
            item_count,
            totals,
            payload_end,
            compression_codec,
            verify_checksums,
            budget,
            cache: GroupCache {
                groups: HashMap::new(),
                order: VecDeque::new(),
                used_bytes: 0,
                max_bytes: max_cached_bytes,
            },
        })
    }

    pub(crate) fn read_all(&self) -> IonResult<Vec<Metadatum>> {
        let mut rows = Vec::new();
        let mut seen = MetaCounts::default();
        for group_index in 0..self.directory.len() as u64 {
            let bytes = self.decode_group(group_index)?;
            let (meta_count, numeric_count, string_count) = read_group_header(&bytes)?;
            seen.add(meta_count, numeric_count, string_count);
            rows.extend(self.parse_group(&bytes, group_index)?);
        }
        self.check_totals(&seen)?;
        Ok(rows)
    }

    fn check_totals(&self, seen: &MetaCounts) -> IonResult<()> {
        if seen.rows != self.totals.rows
            || seen.numeric != self.totals.numeric
            || seen.string != self.totals.string
        {
            return Err(IonError::from(
                "metadata groups: row totals do not match header",
            ));
        }
        Ok(())
    }

    pub(crate) fn read_item(&mut self, item_index: u64) -> IonResult<Vec<Metadatum>> {
        if item_index >= self.item_count {
            return Ok(Vec::new());
        }
        let group_index = group_of_item(item_index, self.group_size);
        let rows = self.get_cached_rows(group_index)?;
        let start = rows.partition_point(|row| (row.item_index as u64) < item_index);
        let end = rows.partition_point(|row| (row.item_index as u64) <= item_index);
        Ok(rows[start..end].to_vec())
    }

    fn get_cached_rows(&mut self, group_index: u64) -> IonResult<Arc<[Metadatum]>> {
        if let Some(group) = self.cache.groups.get(&group_index) {
            return Ok(group.rows.clone());
        }
        let rows = {
            let bytes = self.decode_group(group_index)?;
            let result: Arc<[Metadatum]> = Arc::from(self.parse_group(&bytes, group_index)?);
            result
        };
        let footprint = group_footprint(&rows);
        self.store_in_cache(group_index, rows.clone(), footprint);
        Ok(rows)
    }

    fn store_in_cache(&mut self, group_index: u64, rows: Arc<[Metadatum]>, footprint: usize) {
        debug_assert!(!self.cache.groups.contains_key(&group_index));
        self.cache.used_bytes += footprint;
        self.cache
            .groups
            .insert(group_index, CachedGroup { rows, footprint });
        self.cache.order.push_back(group_index);
        while self.cache.used_bytes > self.cache.max_bytes && self.cache.order.len() > 1 {
            if let Some(oldest) = self.cache.order.pop_front()
                && let Some(removed) = self.cache.groups.remove(&oldest)
            {
                self.cache.used_bytes -= removed.footprint;
            }
        }
    }

    fn decode_group(&self, group_index: u64) -> IonResult<Cow<'_, [u8]>> {
        let entry = self
            .directory
            .get(group_index as usize)
            .ok_or_else(|| IonError::from("metadata groups: group index out of range"))?;
        let payload = self.get_payload(entry)?;
        if self.verify_checksums && crc32fast::hash(payload) != entry.checksum {
            return Err(IonError::from("metadata groups: group checksum mismatch"));
        }
        match self.compression_codec {
            CODEC_NONE => Ok(Cow::Borrowed(payload)),
            CODEC_ZSTD => {
                let size = usize::try_from(entry.uncompressed_size).map_err(|_| {
                    IonError::from("metadata groups: uncompressed size out of range")
                })?;
                let plain = decompress_zstd_allow_aligned_padding(payload, size, self.budget)?;
                Ok(Cow::Owned(plain))
            }
            other => Err(IonError::from(format!(
                "metadata groups: unsupported codec {other}"
            ))),
        }
    }

    fn parse_group(&self, bytes: &[u8], group_index: u64) -> IonResult<Vec<Metadatum>> {
        let (meta_count, numeric_count, string_count) = read_group_header(bytes)?;
        let (item_start, item_end) =
            item_range_of_group(group_index, self.group_size, self.item_count);
        let mut rows = parse_metadata(
            &bytes[META_GROUP_HEADER_SIZE..],
            item_end - item_start,
            meta_count as u64,
            numeric_count as u64,
            string_count as u64,
            CODEC_NONE,
            0,
            self.budget,
        )?;
        let item_base = item_start as u32;
        for row in &mut rows {
            row.item_index += item_base;
        }
        Ok(rows)
    }

    fn get_payload(&self, entry: &MetaGroupEntry) -> IonResult<&[u8]> {
        let start = usize::try_from(entry.payload_offset)
            .map_err(|_| IonError::from("metadata groups: payload offset out of range"))?;
        let size = usize::try_from(entry.payload_size)
            .map_err(|_| IonError::from("metadata groups: payload size out of range"))?;
        let end = start
            .checked_add(size)
            .ok_or_else(|| IonError::from("metadata groups: payload overflows"))?;
        if end > self.payload_end {
            return Err(IonError::from(
                "metadata groups: payload overlaps the directory",
            ));
        }
        self.section
            .get(start..end)
            .ok_or_else(|| IonError::from("metadata groups: payload out of bounds"))
    }
}

fn group_footprint(rows: &[Metadatum]) -> usize {
    let mut total = 0;
    for row in rows {
        total += std::mem::size_of::<Metadatum>();
        if let Some(accession) = &row.accession {
            total += accession.len();
        }
        if let Some(unit) = &row.unit_accession {
            total += unit.len();
        }
        if let MetadatumValue::Text(text) = &row.value {
            total += text.len();
        }
    }
    total
}

fn read_directory(section: &[u8], group_count: u64) -> IonResult<Vec<MetaGroupEntry>> {
    if group_count == 0 {
        return Ok(Vec::new());
    }
    let group_count = usize::try_from(group_count)
        .map_err(|_| IonError::from("metadata groups: group count out of range"))?;
    let directory_size = group_count
        .checked_mul(META_GROUP_ENTRY_SIZE)
        .ok_or_else(|| IonError::from("metadata groups: directory overflows"))?;
    if section.len() < directory_size {
        return Err(IonError::from(
            "metadata groups: section smaller than directory",
        ));
    }
    let directory_start = section.len() - directory_size;
    let mut directory = Vec::with_capacity(group_count);
    for index in 0..group_count {
        let at = directory_start + index * META_GROUP_ENTRY_SIZE;
        directory.push(MetaGroupEntry::read_from(
            &section[at..at + META_GROUP_ENTRY_SIZE],
        ));
    }
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::MetaGroupReader;
    use crate::ion::format::CODEC_NONE;
    use crate::ion::meta_groups::MetaTotals;
    use crate::ion::{DecompressionBudget, IonResult};
    use std::sync::Arc;

    fn no_totals() -> MetaTotals {
        MetaTotals {
            rows: 0,
            numeric: 0,
            string: 0,
            uncompressed: 0,
        }
    }

    fn new_reader(group_count: u64, group_size: u32, item_count: u64) -> IonResult<()> {
        MetaGroupReader::new(
            Arc::from(&[][..]),
            group_count,
            group_size,
            item_count,
            no_totals(),
            CODEC_NONE,
            false,
            DecompressionBudget::default(),
            1024,
        )
        .map(|_| ())
    }

    #[test]
    fn rejects_zero_group_size_when_items_exist() {
        assert!(new_reader(0, 0, 5).is_err());
    }

    #[test]
    fn rejects_group_count_that_disagrees_with_item_count() {
        assert!(new_reader(2, 8192, 5).is_err());
    }

    #[test]
    fn allows_consistent_empty_section() {
        assert!(new_reader(0, 8192, 0).is_ok());
    }
}
