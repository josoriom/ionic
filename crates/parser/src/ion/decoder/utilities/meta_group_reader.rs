use std::borrow::Cow;

use crate::{
    decoder::decode::Metadatum,
    decoder::utilities::{
        common::decompress_zstd_allow_aligned_padding,
        decompression_budget::DecompressionBudget,
        parse_metadata::{HDR_CODEC_NONE, HDR_CODEC_ZSTD, parse_metadata},
    },
    ion::{
        IonError, IonResult,
        meta_groups::{
            META_GROUP_ENTRY_SIZE, META_GROUP_HEADER_SIZE, MetaGroupEntry, item_range_of_group,
            read_group_header,
        },
    },
};

pub(crate) struct MetaGroupReader<'a> {
    section: &'a [u8],
    directory: Vec<MetaGroupEntry>,
    group_size: u32,
    item_count: u64,
    compression_codec: u8,
    verify_checksums: bool,
    budget: DecompressionBudget,
}

impl<'a> MetaGroupReader<'a> {
    pub(crate) fn new(
        section: &'a [u8],
        group_count: u64,
        group_size: u32,
        item_count: u64,
        compression_codec: u8,
        verify_checksums: bool,
        budget: DecompressionBudget,
    ) -> IonResult<Self> {
        if group_count > 0 && group_size == 0 {
            return Err(IonError::from("metadata groups: group size is zero"));
        }
        let directory = read_directory(section, group_count)?;
        Ok(Self {
            section,
            directory,
            group_size,
            item_count,
            compression_codec,
            verify_checksums,
            budget,
        })
    }

    pub(crate) fn read_all(&self) -> IonResult<Vec<Metadatum>> {
        let mut rows = Vec::new();
        for group_index in 0..self.directory.len() as u64 {
            rows.extend(self.read_group(group_index)?);
        }
        Ok(rows)
    }

    fn read_group(&self, group_index: u64) -> IonResult<Vec<Metadatum>> {
        let entry = self
            .directory
            .get(group_index as usize)
            .ok_or_else(|| IonError::from("metadata groups: group index out of range"))?;
        let payload = self.payload_of(entry)?;
        if self.verify_checksums && crc32fast::hash(payload) != entry.checksum {
            return Err(IonError::from("metadata groups: group checksum mismatch"));
        }
        let plain = self.decompress(payload, entry.uncompressed_size)?;
        let (meta_count, numeric_count, string_count) = read_group_header(&plain)?;
        let (item_start, item_end) =
            item_range_of_group(group_index, self.group_size, self.item_count);
        let mut rows = parse_metadata(
            &plain[META_GROUP_HEADER_SIZE..],
            item_end - item_start,
            meta_count as u64,
            numeric_count as u64,
            string_count as u64,
            HDR_CODEC_NONE,
            0,
            self.budget,
        )?;
        let item_base = item_start as u32;
        for row in &mut rows {
            row.item_index += item_base;
        }
        Ok(rows)
    }

    fn payload_of(&self, entry: &MetaGroupEntry) -> IonResult<&'a [u8]> {
        let start = usize::try_from(entry.payload_offset)
            .map_err(|_| IonError::from("metadata groups: payload offset out of range"))?;
        let size = usize::try_from(entry.payload_size)
            .map_err(|_| IonError::from("metadata groups: payload size out of range"))?;
        let end = start
            .checked_add(size)
            .ok_or_else(|| IonError::from("metadata groups: payload overflows"))?;
        self.section
            .get(start..end)
            .ok_or_else(|| IonError::from("metadata groups: payload out of bounds"))
    }

    fn decompress(&self, payload: &'a [u8], uncompressed_size: u64) -> IonResult<Cow<'a, [u8]>> {
        match self.compression_codec {
            HDR_CODEC_NONE => Ok(Cow::Borrowed(payload)),
            HDR_CODEC_ZSTD => {
                let size = usize::try_from(uncompressed_size).map_err(|_| {
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
