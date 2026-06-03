use crate::ion::{
    IonResult,
    format::{FILE_SIGNATURE, FILE_TRAILER, HEADER_SIZE, allow_compression, allow_version},
};

const RESERVED_EXT_SIZE: usize = 632;
const BLOCK_DIRECTORY_ENTRY_SIZE_U64: u64 = 32;

pub const HEADER_FORMAT_VERSION_OFFSET: usize = 9;

#[inline]
pub fn get_version_from_header(bytes: &[u8]) -> Option<u16> {
    let end = HEADER_FORMAT_VERSION_OFFSET + 2;
    if bytes.len() < end {
        return None;
    }
    let mut buf = [0u8; 2];
    buf.copy_from_slice(&bytes[HEADER_FORMAT_VERSION_OFFSET..end]);
    Some(u16::from_le_bytes(buf))
}

pub(crate) fn parse_header(bytes: &[u8]) -> IonResult<Header> {
    if bytes.len() < HEADER_SIZE {
        return Err("header: file too small".into());
    }

    let h = &bytes[..HEADER_SIZE];

    let file_signature = <[u8; 8]>::try_from(&h[0..8]).unwrap();
    if file_signature != FILE_SIGNATURE {
        return Err("header: invalid file signature".into());
    }
    let endianness_flag = h[8];
    if endianness_flag != 0 {
        return Err("header: expected little-endian endianness_flag=0".into());
    }

    let format_version = get_version_from_header(bytes).unwrap();
    allow_version(format_version)?;
    let compression_codec = h[HEADER_CODEC_ID];
    let compression_level = h[HEADER_COMPRESSION_LEVEL];
    allow_compression(compression_codec, compression_level)?;
    let default_array_filter = h[HEADER_ARRAY_FILTER_ID];

    let reserved_14_15 = <[u8; 2]>::try_from(&h[14..16]).unwrap();
    if reserved_14_15 != [0, 0] {
        return Err("header: reserved[2] at 14..16 must be zero".into());
    }

    let target_block_uncompressed_bytes = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_TARGET_BLOCK_SIZE..HEADER_TARGET_BLOCK_SIZE + 8]).unwrap(),
    );

    let off_spec_entries = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_OFFSET_SPEC_ENTRIES..HEADER_OFFSET_SPEC_ENTRIES + 8])
            .unwrap(),
    );
    let len_spec_entries = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_LEN_SPEC_ENTRIES..HEADER_LEN_SPEC_ENTRIES + 8]).unwrap(),
    );
    let off_spec_arrayrefs = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_OFFSET_SPEC_ARRAYREFS..HEADER_OFFSET_SPEC_ARRAYREFS + 8])
            .unwrap(),
    );
    let len_spec_arrayrefs = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_LEN_SPEC_ARRAYREFS..HEADER_LEN_SPEC_ARRAYREFS + 8]).unwrap(),
    );
    let off_chrom_entries = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_OFFSET_CHROM_ENTRIES..HEADER_OFFSET_CHROM_ENTRIES + 8])
            .unwrap(),
    );
    let len_chrom_entries = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_LEN_CHROM_ENTRIES..HEADER_LEN_CHROM_ENTRIES + 8]).unwrap(),
    );
    let off_chrom_arrayrefs = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_OFFSET_CHROM_ARRAYREFS..HEADER_OFFSET_CHROM_ARRAYREFS + 8])
            .unwrap(),
    );
    let len_chrom_arrayrefs = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_LEN_CHROM_ARRAYREFS..HEADER_LEN_CHROM_ARRAYREFS + 8])
            .unwrap(),
    );
    let off_spec_meta = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_OFFSET_SPEC_META..HEADER_OFFSET_SPEC_META + 8]).unwrap(),
    );
    let len_spec_meta = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_LEN_SPEC_META..HEADER_LEN_SPEC_META + 8]).unwrap(),
    );
    let off_chrom_meta = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_OFFSET_CHROM_META..HEADER_OFFSET_CHROM_META + 8]).unwrap(),
    );
    let len_chrom_meta = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_LEN_CHROM_META..HEADER_LEN_CHROM_META + 8]).unwrap(),
    );
    let off_global_meta = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_OFFSET_GLOBAL_META..HEADER_OFFSET_GLOBAL_META + 8]).unwrap(),
    );
    let len_global_meta = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_LEN_GLOBAL_META..HEADER_LEN_GLOBAL_META + 8]).unwrap(),
    );
    let off_spec_container = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_OFFSET_PACKED_SPECTRA..HEADER_OFFSET_PACKED_SPECTRA + 8])
            .unwrap(),
    );
    let len_spec_container = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_LEN_PACKED_SPECTRA..HEADER_LEN_PACKED_SPECTRA + 8]).unwrap(),
    );
    let off_chrom_container = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_OFFSET_PACKED_CHROMS..HEADER_OFFSET_PACKED_CHROMS + 8])
            .unwrap(),
    );
    let len_chrom_container = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_LEN_PACKED_CHROMS..HEADER_LEN_PACKED_CHROMS + 8]).unwrap(),
    );

    let spec_block_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_SPECTRUM_BLOCK_COUNT..HEADER_SPECTRUM_BLOCK_COUNT + 8])
            .unwrap(),
    );
    let chrom_block_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_CHROM_BLOCK_COUNT..HEADER_CHROM_BLOCK_COUNT + 8]).unwrap(),
    );
    let spectrum_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_SPECTRUM_COUNT..HEADER_SPECTRUM_COUNT + 8]).unwrap(),
    );
    let chrom_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_CHROM_COUNT..HEADER_CHROM_COUNT + 8]).unwrap(),
    );

    let spec_meta_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_SPEC_META_ROW_COUNT..HEADER_SPEC_META_ROW_COUNT + 8])
            .unwrap(),
    );
    let spec_meta_numeric_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_SPEC_META_NUMERIC_COUNT..HEADER_SPEC_META_NUMERIC_COUNT + 8])
            .unwrap(),
    );
    let spec_meta_string_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_SPEC_META_STRING_COUNT..HEADER_SPEC_META_STRING_COUNT + 8])
            .unwrap(),
    );
    let chrom_meta_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_CHROM_META_ROW_COUNT..HEADER_CHROM_META_ROW_COUNT + 8])
            .unwrap(),
    );
    let chrom_meta_numeric_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(
            &h[HEADER_CHROM_META_NUMERIC_COUNT..HEADER_CHROM_META_NUMERIC_COUNT + 8],
        )
        .unwrap(),
    );
    let chrom_meta_string_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_CHROM_META_STRING_COUNT..HEADER_CHROM_META_STRING_COUNT + 8])
            .unwrap(),
    );
    let global_meta_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_GLOBAL_META_ROW_COUNT..HEADER_GLOBAL_META_ROW_COUNT + 8])
            .unwrap(),
    );
    let global_meta_numeric_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(
            &h[HEADER_GLOBAL_META_NUMERIC_COUNT..HEADER_GLOBAL_META_NUMERIC_COUNT + 8],
        )
        .unwrap(),
    );
    let global_meta_string_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(
            &h[HEADER_GLOBAL_META_STRING_COUNT..HEADER_GLOBAL_META_STRING_COUNT + 8],
        )
        .unwrap(),
    );
    let spec_array_type_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_SPEC_ARRAY_TYPE_COUNT..HEADER_SPEC_ARRAY_TYPE_COUNT + 8])
            .unwrap(),
    );
    let chrom_array_type_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_CHROM_ARRAY_TYPE_COUNT..HEADER_CHROM_ARRAY_TYPE_COUNT + 8])
            .unwrap(),
    );

    let spec_meta_uncompressed_bytes = u64::from_le_bytes(
        <[u8; 8]>::try_from(
            &h[HEADER_SPEC_META_UNCOMPRESSED_SIZE..HEADER_SPEC_META_UNCOMPRESSED_SIZE + 8],
        )
        .unwrap(),
    );
    let chrom_meta_uncompressed_bytes = u64::from_le_bytes(
        <[u8; 8]>::try_from(
            &h[HEADER_CHROM_META_UNCOMPRESSED_SIZE..HEADER_CHROM_META_UNCOMPRESSED_SIZE + 8],
        )
        .unwrap(),
    );
    let global_meta_uncompressed_bytes = u64::from_le_bytes(
        <[u8; 8]>::try_from(
            &h[HEADER_GLOBAL_META_UNCOMPRESSED_SIZE..HEADER_GLOBAL_META_UNCOMPRESSED_SIZE + 8],
        )
        .unwrap(),
    );

    let off_spec_summary = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_OFF_SPEC_SUMMARY..HEADER_OFF_SPEC_SUMMARY + 8]).unwrap(),
    );
    let len_spec_summary = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_LEN_SPEC_SUMMARY..HEADER_LEN_SPEC_SUMMARY + 8]).unwrap(),
    );
    let off_chrom_summary = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_OFF_CHROM_SUMMARY..HEADER_OFF_CHROM_SUMMARY + 8]).unwrap(),
    );
    let len_chrom_summary = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_LEN_CHROM_SUMMARY..HEADER_LEN_CHROM_SUMMARY + 8]).unwrap(),
    );
    let total_file_size = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_TOTAL_FILE_SIZE..HEADER_TOTAL_FILE_SIZE + 8]).unwrap(),
    );

    let meta_group_size_stored = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_META_GROUP_SIZE..HEADER_META_GROUP_SIZE + 8]).unwrap(),
    );
    if meta_group_size_stored > u32::MAX as u64 {
        return Err("header: meta_group_size exceeds u32 range".into());
    }
    let meta_group_size = meta_group_size_stored as u32;
    let spec_meta_group_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_SPEC_META_GROUP_COUNT..HEADER_SPEC_META_GROUP_COUNT + 8])
            .unwrap(),
    );
    let chrom_meta_group_count = u64::from_le_bytes(
        <[u8; 8]>::try_from(&h[HEADER_CHROM_META_GROUP_COUNT..HEADER_CHROM_META_GROUP_COUNT + 8])
            .unwrap(),
    );

    let reserved_ext = <[u8; RESERVED_EXT_SIZE]>::try_from(&h[376..1008]).unwrap();
    if reserved_ext.iter().any(|&b| b != 0) {
        return Err("header: reserved_ext must be all zeros".into());
    }

    let spec_meta_crc32 = u32::from_le_bytes(
        <[u8; 4]>::try_from(&h[HEADER_SPEC_META_CRC32..HEADER_SPEC_META_CRC32 + 4]).unwrap(),
    );
    let chrom_meta_crc32 = u32::from_le_bytes(
        <[u8; 4]>::try_from(&h[HEADER_CHROM_META_CRC32..HEADER_CHROM_META_CRC32 + 4]).unwrap(),
    );
    let global_meta_crc32 = u32::from_le_bytes(
        <[u8; 4]>::try_from(&h[HEADER_GLOBAL_META_CRC32..HEADER_GLOBAL_META_CRC32 + 4]).unwrap(),
    );
    let header_crc32 =
        u32::from_le_bytes(<[u8; 4]>::try_from(&h[HEADER_CRC32..HEADER_CRC32 + 4]).unwrap());

    let header = Header {
        file_signature,
        endianness_flag,
        format_version,
        compression_codec,
        compression_level,
        default_array_filter,
        target_block_uncompressed_bytes,
        off_spec_entries,
        len_spec_entries,
        off_spec_arrayrefs,
        len_spec_arrayrefs,
        off_chrom_entries,
        len_chrom_entries,
        off_chrom_arrayrefs,
        len_chrom_arrayrefs,
        off_spec_meta,
        len_spec_meta,
        off_chrom_meta,
        len_chrom_meta,
        off_global_meta,
        len_global_meta,
        off_spec_container,
        len_spec_container,
        off_chrom_container,
        len_chrom_container,
        spec_block_count,
        chrom_block_count,
        spectrum_count,
        chrom_count,
        spec_meta_count,
        spec_meta_numeric_count,
        spec_meta_string_count,
        chrom_meta_count,
        chrom_meta_numeric_count,
        chrom_meta_string_count,
        global_meta_count,
        global_meta_numeric_count,
        global_meta_string_count,
        spec_array_type_count,
        chrom_array_type_count,
        spec_meta_uncompressed_bytes,
        chrom_meta_uncompressed_bytes,
        global_meta_uncompressed_bytes,
        off_spec_summary,
        len_spec_summary,
        off_chrom_summary,
        len_chrom_summary,
        total_file_size,
        meta_group_size,
        spec_meta_group_count,
        chrom_meta_group_count,
        reserved_ext,
        spec_meta_crc32,
        chrom_meta_crc32,
        global_meta_crc32,
        header_crc32,
    };

    let (passed, failures) = validate_file_integrity(bytes, &header);
    if !passed {
        return Err(format!(
            "header: file integrity validation failed ({} check(s)):\n{}",
            failures.len(),
            failures.join("\n")
        )
        .into());
    }

    Ok(header)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) file_signature: [u8; 8],
    pub(crate) endianness_flag: u8,
    pub(crate) format_version: u16,
    pub(crate) compression_codec: u8,
    pub(crate) compression_level: u8,
    pub(crate) default_array_filter: u8,
    pub(crate) target_block_uncompressed_bytes: u64,
    pub(crate) off_spec_summary: u64,
    pub(crate) len_spec_summary: u64,
    pub(crate) off_chrom_summary: u64,
    pub(crate) len_chrom_summary: u64,
    pub(crate) off_spec_entries: u64,
    pub(crate) len_spec_entries: u64,
    pub(crate) off_spec_arrayrefs: u64,
    pub(crate) len_spec_arrayrefs: u64,
    pub(crate) off_chrom_entries: u64,
    pub(crate) len_chrom_entries: u64,
    pub(crate) off_chrom_arrayrefs: u64,
    pub(crate) len_chrom_arrayrefs: u64,
    pub(crate) off_spec_meta: u64,
    pub(crate) len_spec_meta: u64,
    pub(crate) off_chrom_meta: u64,
    pub(crate) len_chrom_meta: u64,
    pub(crate) off_global_meta: u64,
    pub(crate) len_global_meta: u64,
    pub(crate) off_spec_container: u64,
    pub(crate) len_spec_container: u64,
    pub(crate) off_chrom_container: u64,
    pub(crate) len_chrom_container: u64,
    pub(crate) spec_block_count: u64,
    pub(crate) chrom_block_count: u64,
    pub(crate) spectrum_count: u64,
    pub(crate) chrom_count: u64,
    pub(crate) spec_meta_count: u64,
    pub(crate) spec_meta_numeric_count: u64,
    pub(crate) spec_meta_string_count: u64,
    pub(crate) chrom_meta_count: u64,
    pub(crate) chrom_meta_numeric_count: u64,
    pub(crate) chrom_meta_string_count: u64,
    pub(crate) global_meta_count: u64,
    pub(crate) global_meta_numeric_count: u64,
    pub(crate) global_meta_string_count: u64,
    pub(crate) spec_array_type_count: u64,
    pub(crate) chrom_array_type_count: u64,
    pub(crate) spec_meta_uncompressed_bytes: u64,
    pub(crate) chrom_meta_uncompressed_bytes: u64,
    pub(crate) global_meta_uncompressed_bytes: u64,
    pub(crate) total_file_size: u64,
    pub(crate) meta_group_size: u32,
    pub(crate) spec_meta_group_count: u64,
    pub(crate) chrom_meta_group_count: u64,
    pub(crate) reserved_ext: [u8; RESERVED_EXT_SIZE],
    pub(crate) spec_meta_crc32: u32,
    pub(crate) chrom_meta_crc32: u32,
    pub(crate) global_meta_crc32: u32,
    pub(crate) header_crc32: u32,
}

pub(crate) const HEADER_CODEC_ID: usize = 11;
pub(crate) const HEADER_COMPRESSION_LEVEL: usize = 12;
pub(crate) const HEADER_ARRAY_FILTER_ID: usize = 13;
pub(crate) const HEADER_TARGET_BLOCK_SIZE: usize = 16;
pub(crate) const HEADER_OFF_SPEC_SUMMARY: usize = 24;
pub(crate) const HEADER_LEN_SPEC_SUMMARY: usize = 32;
pub(crate) const HEADER_OFFSET_SPEC_ENTRIES: usize = 40;
pub(crate) const HEADER_LEN_SPEC_ENTRIES: usize = 48;
pub(crate) const HEADER_OFFSET_SPEC_ARRAYREFS: usize = 56;
pub(crate) const HEADER_LEN_SPEC_ARRAYREFS: usize = 64;
pub(crate) const HEADER_OFF_CHROM_SUMMARY: usize = 72;
pub(crate) const HEADER_LEN_CHROM_SUMMARY: usize = 80;
pub(crate) const HEADER_OFFSET_CHROM_ENTRIES: usize = 88;
pub(crate) const HEADER_LEN_CHROM_ENTRIES: usize = 96;
pub(crate) const HEADER_OFFSET_CHROM_ARRAYREFS: usize = 104;
pub(crate) const HEADER_LEN_CHROM_ARRAYREFS: usize = 112;
pub(crate) const HEADER_OFFSET_SPEC_META: usize = 120;
pub(crate) const HEADER_LEN_SPEC_META: usize = 128;
pub(crate) const HEADER_OFFSET_CHROM_META: usize = 136;
pub(crate) const HEADER_LEN_CHROM_META: usize = 144;
pub(crate) const HEADER_OFFSET_GLOBAL_META: usize = 152;
pub(crate) const HEADER_LEN_GLOBAL_META: usize = 160;
pub(crate) const HEADER_OFFSET_PACKED_SPECTRA: usize = 168;
pub(crate) const HEADER_LEN_PACKED_SPECTRA: usize = 176;
pub(crate) const HEADER_OFFSET_PACKED_CHROMS: usize = 184;
pub(crate) const HEADER_LEN_PACKED_CHROMS: usize = 192;
pub(crate) const HEADER_SPECTRUM_BLOCK_COUNT: usize = 200;
pub(crate) const HEADER_CHROM_BLOCK_COUNT: usize = 208;
pub(crate) const HEADER_SPECTRUM_COUNT: usize = 216;
pub(crate) const HEADER_CHROM_COUNT: usize = 224;
pub(crate) const HEADER_SPEC_META_ROW_COUNT: usize = 232;
pub(crate) const HEADER_SPEC_META_NUMERIC_COUNT: usize = 240;
pub(crate) const HEADER_SPEC_META_STRING_COUNT: usize = 248;
pub(crate) const HEADER_CHROM_META_ROW_COUNT: usize = 256;
pub(crate) const HEADER_CHROM_META_NUMERIC_COUNT: usize = 264;
pub(crate) const HEADER_CHROM_META_STRING_COUNT: usize = 272;
pub(crate) const HEADER_GLOBAL_META_ROW_COUNT: usize = 280;
pub(crate) const HEADER_GLOBAL_META_NUMERIC_COUNT: usize = 288;
pub(crate) const HEADER_GLOBAL_META_STRING_COUNT: usize = 296;
pub(crate) const HEADER_SPEC_ARRAY_TYPE_COUNT: usize = 304;
pub(crate) const HEADER_CHROM_ARRAY_TYPE_COUNT: usize = 312;
pub(crate) const HEADER_SPEC_META_UNCOMPRESSED_SIZE: usize = 320;
pub(crate) const HEADER_CHROM_META_UNCOMPRESSED_SIZE: usize = 328;
pub(crate) const HEADER_GLOBAL_META_UNCOMPRESSED_SIZE: usize = 336;
pub(crate) const HEADER_TOTAL_FILE_SIZE: usize = 344;
pub(crate) const HEADER_META_GROUP_SIZE: usize = 352;
pub(crate) const HEADER_SPEC_META_GROUP_COUNT: usize = 360;
pub(crate) const HEADER_CHROM_META_GROUP_COUNT: usize = 368;
pub(crate) const HEADER_SPEC_META_CRC32: usize = 1008;
pub(crate) const HEADER_CHROM_META_CRC32: usize = 1012;
pub(crate) const HEADER_GLOBAL_META_CRC32: usize = 1016;
pub(crate) const HEADER_CRC32: usize = 1020;

pub(crate) fn validate_file_integrity(bytes: &[u8], h: &Header) -> (bool, Vec<String>) {
    let mut failures = Vec::new();
    let file_len = bytes.len() as u64;

    if h.file_signature != FILE_SIGNATURE {
        failures.push(format!(
            "condition 1: invalid file_signature (stored={:?}, expected=\"START\\0\\0\\0\")",
            String::from_utf8_lossy(&h.file_signature)
        ));
    }

    let computed_header_crc = crc32fast::hash(&bytes[0..1020]);
    if computed_header_crc != h.header_crc32 {
        failures.push(format!(
            "condition 2: header_crc32 mismatch (stored={:#010x}, computed={:#010x})",
            h.header_crc32, computed_header_crc
        ));
    }

    if bytes.len() < FILE_TRAILER.len()
        || &bytes[bytes.len() - FILE_TRAILER.len()..] != FILE_TRAILER
    {
        failures.push("condition 3: missing or invalid file trailer".to_string());
    }

    if file_len != h.total_file_size {
        failures.push(format!(
            "condition 4: file size mismatch (actual={file_len}, expected={})",
            h.total_file_size
        ));
    }

    match h.spec_block_count.checked_mul(BLOCK_DIRECTORY_ENTRY_SIZE_U64) {
        None => failures.push(format!(
            "condition 5: spec_block_count ({}) × {BLOCK_DIRECTORY_ENTRY_SIZE_U64} overflows u64",
            h.spec_block_count
        )),
        Some(directory_bytes) if directory_bytes > h.len_spec_container => failures.push(format!(
            "condition 5: spec_block_count × {BLOCK_DIRECTORY_ENTRY_SIZE_U64} ({directory_bytes}) > len_spec_container ({})",
            h.len_spec_container
        )),
        _ => {}
    }

    match h.chrom_block_count.checked_mul(BLOCK_DIRECTORY_ENTRY_SIZE_U64) {
        None => failures.push(format!(
            "condition 6: chrom_block_count ({}) × {BLOCK_DIRECTORY_ENTRY_SIZE_U64} overflows u64",
            h.chrom_block_count
        )),
        Some(directory_bytes) if directory_bytes > h.len_chrom_container => failures.push(format!(
            "condition 6: chrom_block_count × {BLOCK_DIRECTORY_ENTRY_SIZE_U64} ({directory_bytes}) > len_chrom_container ({})",
            h.len_chrom_container
        )),
        _ => {}
    }

    let trailer_start = h.total_file_size.saturating_sub(8);
    let sections: &[(&str, u64, u64)] = &[
        ("spec_entries", h.off_spec_entries, h.len_spec_entries),
        ("spec_arrayrefs", h.off_spec_arrayrefs, h.len_spec_arrayrefs),
        ("chrom_entries", h.off_chrom_entries, h.len_chrom_entries),
        (
            "chrom_arrayrefs",
            h.off_chrom_arrayrefs,
            h.len_chrom_arrayrefs,
        ),
        ("spec_meta", h.off_spec_meta, h.len_spec_meta),
        ("chrom_meta", h.off_chrom_meta, h.len_chrom_meta),
        ("global_meta", h.off_global_meta, h.len_global_meta),
        (
            "container_spect",
            h.off_spec_container,
            h.len_spec_container,
        ),
        (
            "container_chrom",
            h.off_chrom_container,
            h.len_chrom_container,
        ),
        ("spec_summary", h.off_spec_summary, h.len_spec_summary),
        ("chrom_summary", h.off_chrom_summary, h.len_chrom_summary),
    ];

    for &(name, off, len) in sections {
        match off.checked_add(len) {
            None => failures.push(format!("condition 7: {name} offset+length overflows u64")),
            Some(end) if end > trailer_start => failures.push(format!(
                "condition 7: {name} end ({end}) exceeds trailer_start ({trailer_start})"
            )),
            _ => {}
        }
    }

    let mut sorted: Vec<(&str, u64, u64)> = sections
        .iter()
        .copied()
        .filter(|&(_, _, len)| len > 0)
        .collect();
    sorted.sort_by_key(|&(_, off, _)| off);
    for i in 1..sorted.len() {
        let (prev_name, prev_off, prev_len) = sorted[i - 1];
        let (curr_name, curr_off, _) = sorted[i];
        match prev_off.checked_add(prev_len) {
            None => failures.push(format!(
                "condition 8: section {prev_name} offset+length overflows u64"
            )),
            Some(prev_end) if prev_end > curr_off => failures.push(format!(
                "condition 8: sections {prev_name} and {curr_name} overlap \
                 ({prev_name} ends at {prev_end}, {curr_name} starts at {curr_off})"
            )),
            _ => {}
        }
    }

    for &(name, off, len) in sections {
        if len > 0 && off % 8 != 0 {
            failures.push(format!(
                "condition 9: {name} offset ({off}) is not 8-byte aligned"
            ));
        }
        if len > 0 && off < HEADER_SIZE as u64 {
            failures.push(format!(
                "condition 9: {name} offset ({off}) is inside the {HEADER_SIZE}-byte header"
            ));
        }
    }

    for (cond, name, off, len, stored) in [
        (
            10,
            "spec_meta",
            h.off_spec_meta,
            h.len_spec_meta,
            h.spec_meta_crc32,
        ),
        (
            11,
            "chrom_meta",
            h.off_chrom_meta,
            h.len_chrom_meta,
            h.chrom_meta_crc32,
        ),
        (
            12,
            "global_meta",
            h.off_global_meta,
            h.len_global_meta,
            h.global_meta_crc32,
        ),
    ] {
        match resolve_section(bytes, off, len) {
            None => failures.push(format!("condition {cond}: {name} section out of bounds")),
            Some(section) => {
                let computed = crc32fast::hash(section);
                if computed != stored {
                    failures.push(format!(
                        "condition {cond}: {name}_crc32 mismatch (stored={:#010x}, computed={:#010x})",
                        stored, computed
                    ));
                }
            }
        }
    }

    enforce_count_bounds(&mut failures, h, file_len);

    let passed = failures.is_empty();
    (passed, failures)
}

#[inline]
fn resolve_section(bytes: &[u8], off: u64, len: u64) -> Option<&[u8]> {
    let start = usize::try_from(off).ok()?;
    let end = usize::try_from(off.checked_add(len)?).ok()?;
    bytes.get(start..end)
}

fn enforce_count_bounds(failures: &mut Vec<String>, h: &Header, file_len: u64) {
    let checks: &[(&str, u64)] = &[
        ("spec_block_count", h.spec_block_count),
        ("chrom_block_count", h.chrom_block_count),
        ("spectrum_count", h.spectrum_count),
        ("chrom_count", h.chrom_count),
        ("spec_meta_count", h.spec_meta_count),
        ("spec_meta_numeric_count", h.spec_meta_numeric_count),
        ("spec_meta_string_count", h.spec_meta_string_count),
        ("chrom_meta_count", h.chrom_meta_count),
        ("chrom_meta_numeric_count", h.chrom_meta_numeric_count),
        ("chrom_meta_string_count", h.chrom_meta_string_count),
        ("global_meta_count", h.global_meta_count),
        ("global_meta_numeric_count", h.global_meta_numeric_count),
        ("global_meta_string_count", h.global_meta_string_count),
        ("spec_array_type_count", h.spec_array_type_count),
        ("chrom_array_type_count", h.chrom_array_type_count),
    ];
    for &(name, count) in checks {
        if count > file_len {
            failures.push(format!(
                "condition 13: {name} ({count}) exceeds file size ({file_len})"
            ));
        }
    }
}

#[cfg(test)]
mod inline_tests {
    use super::*;
    use crate::ion::format::CURRENT_VERSION;

    #[test]
    fn get_version_returns_none_on_short_buffer() {
        let too_short = [0u8; HEADER_FORMAT_VERSION_OFFSET + 1];
        assert_eq!(get_version_from_header(&too_short), None);
    }

    #[test]
    fn get_version_reads_little_endian_word() {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[HEADER_FORMAT_VERSION_OFFSET..HEADER_FORMAT_VERSION_OFFSET + 2]
            .copy_from_slice(&CURRENT_VERSION.to_le_bytes());
        assert_eq!(get_version_from_header(&bytes), Some(CURRENT_VERSION));
    }

    #[test]
    fn get_version_handles_max_u16() {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[HEADER_FORMAT_VERSION_OFFSET..HEADER_FORMAT_VERSION_OFFSET + 2]
            .copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(get_version_from_header(&bytes), Some(u16::MAX));
    }

    #[test]
    fn get_version_handles_exact_minimum_buffer_length() {
        let bytes = [0u8; HEADER_FORMAT_VERSION_OFFSET + 2];
        assert_eq!(get_version_from_header(&bytes), Some(0));
    }

    #[test]
    fn header_offsets_match_format_contract() {
        assert_eq!(HEADER_FORMAT_VERSION_OFFSET, 9);
        assert_eq!(HEADER_CODEC_ID, 11);
        assert_eq!(HEADER_COMPRESSION_LEVEL, 12);
        assert_eq!(HEADER_ARRAY_FILTER_ID, 13);
        assert_eq!(HEADER_TARGET_BLOCK_SIZE, 16);
        assert_eq!(HEADER_OFF_SPEC_SUMMARY, 24);
        assert_eq!(HEADER_LEN_SPEC_SUMMARY, 32);
        assert_eq!(HEADER_OFFSET_SPEC_ENTRIES, 40);
        assert_eq!(HEADER_LEN_SPEC_ENTRIES, 48);
        assert_eq!(HEADER_OFFSET_SPEC_ARRAYREFS, 56);
        assert_eq!(HEADER_LEN_SPEC_ARRAYREFS, 64);
        assert_eq!(HEADER_OFF_CHROM_SUMMARY, 72);
        assert_eq!(HEADER_LEN_CHROM_SUMMARY, 80);
        assert_eq!(HEADER_OFFSET_CHROM_ENTRIES, 88);
        assert_eq!(HEADER_LEN_CHROM_ENTRIES, 96);
        assert_eq!(HEADER_OFFSET_CHROM_ARRAYREFS, 104);
        assert_eq!(HEADER_LEN_CHROM_ARRAYREFS, 112);
        assert_eq!(HEADER_OFFSET_SPEC_META, 120);
        assert_eq!(HEADER_LEN_SPEC_META, 128);
        assert_eq!(HEADER_OFFSET_CHROM_META, 136);
        assert_eq!(HEADER_LEN_CHROM_META, 144);
        assert_eq!(HEADER_OFFSET_GLOBAL_META, 152);
        assert_eq!(HEADER_LEN_GLOBAL_META, 160);
        assert_eq!(HEADER_OFFSET_PACKED_SPECTRA, 168);
        assert_eq!(HEADER_LEN_PACKED_SPECTRA, 176);
        assert_eq!(HEADER_OFFSET_PACKED_CHROMS, 184);
        assert_eq!(HEADER_LEN_PACKED_CHROMS, 192);
        assert_eq!(HEADER_SPECTRUM_BLOCK_COUNT, 200);
        assert_eq!(HEADER_CHROM_BLOCK_COUNT, 208);
        assert_eq!(HEADER_SPECTRUM_COUNT, 216);
        assert_eq!(HEADER_CHROM_COUNT, 224);
        assert_eq!(HEADER_SPEC_META_ROW_COUNT, 232);
        assert_eq!(HEADER_SPEC_META_NUMERIC_COUNT, 240);
        assert_eq!(HEADER_SPEC_META_STRING_COUNT, 248);
        assert_eq!(HEADER_CHROM_META_ROW_COUNT, 256);
        assert_eq!(HEADER_CHROM_META_NUMERIC_COUNT, 264);
        assert_eq!(HEADER_CHROM_META_STRING_COUNT, 272);
        assert_eq!(HEADER_GLOBAL_META_ROW_COUNT, 280);
        assert_eq!(HEADER_GLOBAL_META_NUMERIC_COUNT, 288);
        assert_eq!(HEADER_GLOBAL_META_STRING_COUNT, 296);
        assert_eq!(HEADER_SPEC_ARRAY_TYPE_COUNT, 304);
        assert_eq!(HEADER_CHROM_ARRAY_TYPE_COUNT, 312);
        assert_eq!(HEADER_SPEC_META_UNCOMPRESSED_SIZE, 320);
        assert_eq!(HEADER_CHROM_META_UNCOMPRESSED_SIZE, 328);
        assert_eq!(HEADER_GLOBAL_META_UNCOMPRESSED_SIZE, 336);
        assert_eq!(HEADER_TOTAL_FILE_SIZE, 344);
        assert_eq!(HEADER_META_GROUP_SIZE, 352);
        assert_eq!(HEADER_SPEC_META_GROUP_COUNT, 360);
        assert_eq!(HEADER_CHROM_META_GROUP_COUNT, 368);
        assert_eq!(HEADER_SPEC_META_CRC32, 1008);
        assert_eq!(HEADER_CHROM_META_CRC32, 1012);
        assert_eq!(HEADER_GLOBAL_META_CRC32, 1016);
        assert_eq!(HEADER_CRC32, 1020);
    }
}
