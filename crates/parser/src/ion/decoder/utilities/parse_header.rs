use crate::encoder::encode::FILTER_INDEX_RECORD_SIZE;

const HEADER_SIZE: usize = 1024;
const RESERVED_EXT_SIZE: usize = 672;

pub(crate) fn parse_header(bytes: &[u8]) -> Result<Header, String> {
    if bytes.len() < HEADER_SIZE {
        return Err("header: file too small".to_string());
    }

    let mut r = Reader::new(&bytes[..HEADER_SIZE]);

    let file_signature = r.read_arr::<8>("file_signature")?;
    if &file_signature != b"START\0\0\0" {
        return Err("header: invalid file_signature (expected \"START\\0\\0\\0\")".into());
    }

    let endianness_flag = r.read_u8("endianness_flag")?;
    if endianness_flag != 0 {
        return Err("header: expected little-endian endianness_flag=0".into());
    }

    let format_version = r.read_u16_le("format_version")?;
    let compression_codec = r.read_u8("compression_codec")?;
    let compression_level = r.read_u8("compression_level")?;
    let array_filter = r.read_u8("array_filter")?;

    let reserved_14_15 = r.read_arr::<2>("reserved_14_15")?;
    if reserved_14_15 != [0, 0] {
        return Err("header: reserved[2] at 14..16 must be zero".into());
    }

    let target_block_uncompressed_bytes = r.read_u64_le("target_block_uncompressed_bytes")?;

    let off_spec_entries = r.read_u64_le("off_spec_entries")?;
    let len_spec_entries = r.read_u64_le("len_spec_entries")?;
    let off_spec_arrayrefs = r.read_u64_le("off_spec_arrayrefs")?;
    let len_spec_arrayrefs = r.read_u64_le("len_spec_arrayrefs")?;
    let off_chrom_entries = r.read_u64_le("off_chrom_entries")?;
    let len_chrom_entries = r.read_u64_le("len_chrom_entries")?;
    let off_chrom_arrayrefs = r.read_u64_le("off_chrom_arrayrefs")?;
    let len_chrom_arrayrefs = r.read_u64_le("len_chrom_arrayrefs")?;
    let off_spec_meta = r.read_u64_le("off_spec_meta")?;
    let len_spec_meta = r.read_u64_le("len_spec_meta")?;
    let off_chrom_meta = r.read_u64_le("off_chrom_meta")?;
    let len_chrom_meta = r.read_u64_le("len_chrom_meta")?;
    let off_global_meta = r.read_u64_le("off_global_meta")?;
    let len_global_meta = r.read_u64_le("len_global_meta")?;
    let off_container_spect = r.read_u64_le("off_container_spect")?;
    let len_container_spect = r.read_u64_le("len_container_spect")?;
    let off_container_chrom = r.read_u64_le("off_container_chrom")?;
    let len_container_chrom = r.read_u64_le("len_container_chrom")?;

    let block_count_spect = r.read_u64_le("block_count_spect")?;
    let block_count_chrom = r.read_u64_le("block_count_chrom")?;
    let spectrum_count = r.read_u64_le("spectrum_count")?;
    let chrom_count = r.read_u64_le("chrom_count")?;

    let spec_meta_count = r.read_u64_le("spec_meta_count")?;
    let spec_meta_num_count = r.read_u64_le("spec_meta_num_count")?;
    let spec_meta_str_count = r.read_u64_le("spec_meta_str_count")?;
    let chrom_meta_count = r.read_u64_le("chrom_meta_count")?;
    let chrom_meta_num_count = r.read_u64_le("chrom_meta_num_count")?;
    let chrom_meta_str_count = r.read_u64_le("chrom_meta_str_count")?;
    let global_meta_count = r.read_u64_le("global_meta_count")?;
    let global_meta_num_count = r.read_u64_le("global_meta_num_count")?;
    let global_meta_str_count = r.read_u64_le("global_meta_str_count")?;
    let spect_array_count = r.read_u64_le("spect_array_count")?;
    let chrom_array_count = r.read_u64_le("chrom_array_count")?;

    let spec_meta_uncompressed_bytes = r.read_u64_le("spec_meta_uncompressed_bytes")?;
    let chrom_meta_uncompressed_bytes = r.read_u64_le("chrom_meta_uncompressed_bytes")?;
    let global_meta_uncompressed_bytes = r.read_u64_le("global_meta_uncompressed_bytes")?;

    let off_filter_index = r.read_u64_le("off_filter_index")?;
    let len_filter_index = r.read_u64_le("len_filter_index")?;
    let total_file_size = r.read_u64_le("total_file_size")?;

    let reserved_ext = r.read_arr::<RESERVED_EXT_SIZE>("reserved_ext")?;
    if reserved_ext.iter().any(|&b| b != 0) {
        return Err("header: reserved_ext must be all zeros".into());
    }

    let spec_meta_crc32 = r.read_u32_le("spec_meta_crc32")?;
    let chrom_meta_crc32 = r.read_u32_le("chrom_meta_crc32")?;
    let global_meta_crc32 = r.read_u32_le("global_meta_crc32")?;
    let header_crc32 = r.read_u32_le("header_crc32")?;

    debug_assert_eq!(r.pos, HEADER_SIZE);

    let header = Header {
        file_signature,
        endianness_flag,
        format_version,
        compression_codec,
        compression_level,
        array_filter,
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
        off_container_spect,
        len_container_spect,
        off_container_chrom,
        len_container_chrom,
        block_count_spect,
        block_count_chrom,
        spectrum_count,
        chrom_count,
        spec_meta_count,
        spec_meta_num_count,
        spec_meta_str_count,
        chrom_meta_count,
        chrom_meta_num_count,
        chrom_meta_str_count,
        global_meta_count,
        global_meta_num_count,
        global_meta_str_count,
        spect_array_count,
        chrom_array_count,
        spec_meta_uncompressed_bytes,
        chrom_meta_uncompressed_bytes,
        global_meta_uncompressed_bytes,
        off_filter_index,
        len_filter_index,
        total_file_size,
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
        ));
    }

    Ok(header)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub file_signature: [u8; 8],
    pub endianness_flag: u8,
    pub format_version: u16,
    pub compression_codec: u8,
    pub compression_level: u8,
    pub array_filter: u8,
    pub target_block_uncompressed_bytes: u64,
    pub off_spec_entries: u64,
    pub len_spec_entries: u64,
    pub off_spec_arrayrefs: u64,
    pub len_spec_arrayrefs: u64,
    pub off_chrom_entries: u64,
    pub len_chrom_entries: u64,
    pub off_chrom_arrayrefs: u64,
    pub len_chrom_arrayrefs: u64,
    pub off_spec_meta: u64,
    pub len_spec_meta: u64,
    pub off_chrom_meta: u64,
    pub len_chrom_meta: u64,
    pub off_global_meta: u64,
    pub len_global_meta: u64,
    pub off_container_spect: u64,
    pub len_container_spect: u64,
    pub off_container_chrom: u64,
    pub len_container_chrom: u64,
    pub block_count_spect: u64,
    pub block_count_chrom: u64,
    pub spectrum_count: u64,
    pub chrom_count: u64,
    pub spec_meta_count: u64,
    pub spec_meta_num_count: u64,
    pub spec_meta_str_count: u64,
    pub chrom_meta_count: u64,
    pub chrom_meta_num_count: u64,
    pub chrom_meta_str_count: u64,
    pub global_meta_count: u64,
    pub global_meta_num_count: u64,
    pub global_meta_str_count: u64,
    pub spect_array_count: u64,
    pub chrom_array_count: u64,
    pub spec_meta_uncompressed_bytes: u64,
    pub chrom_meta_uncompressed_bytes: u64,
    pub global_meta_uncompressed_bytes: u64,
    pub off_filter_index: u64,
    pub len_filter_index: u64,
    pub total_file_size: u64,
    pub reserved_ext: [u8; RESERVED_EXT_SIZE],
    pub spec_meta_crc32: u32,
    pub chrom_meta_crc32: u32,
    pub global_meta_crc32: u32,
    pub header_crc32: u32,
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    #[inline]
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    #[inline]
    fn need(&self, n: usize, field: &str) -> Result<(), String> {
        if self.pos + n <= self.bytes.len() {
            Ok(())
        } else {
            Err(format!(
                "header: not enough bytes for {field} at offset {} (need {n}, have {})",
                self.pos,
                self.bytes.len().saturating_sub(self.pos)
            ))
        }
    }

    #[inline]
    fn read_u8(&mut self, field: &str) -> Result<u8, String> {
        self.need(1, field)?;
        let v = self.bytes[self.pos];
        self.pos += 1;
        Ok(v)
    }

    #[inline]
    fn read_u16_le(&mut self, field: &str) -> Result<u16, String> {
        self.need(2, field)?;
        let v = u16::from_le_bytes(self.bytes[self.pos..self.pos + 2].try_into().unwrap());
        self.pos += 2;
        Ok(v)
    }

    #[inline]
    fn read_u32_le(&mut self, field: &str) -> Result<u32, String> {
        self.need(4, field)?;
        let v = u32::from_le_bytes(self.bytes[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    #[inline]
    fn read_u64_le(&mut self, field: &str) -> Result<u64, String> {
        self.need(8, field)?;
        let v = u64::from_le_bytes(self.bytes[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }

    #[inline]
    fn read_arr<const N: usize>(&mut self, field: &str) -> Result<[u8; N], String> {
        self.need(N, field)?;
        let v: [u8; N] = self.bytes[self.pos..self.pos + N].try_into().unwrap();
        self.pos += N;
        Ok(v)
    }
}

pub(crate) const HEADER_FORMAT_VERSION: usize = 9;
pub(crate) const HEADER_CODEC_ID: usize = 11;
pub(crate) const HEADER_COMPRESSION_LEVEL: usize = 12;
pub(crate) const HEADER_ARRAY_FILTER_ID: usize = 13;
pub(crate) const HEADER_TARGET_BLOCK_SIZE: usize = 16;
pub(crate) const HEADER_OFFSET_SPEC_ENTRIES: usize = 24;
pub(crate) const HEADER_LEN_SPEC_ENTRIES: usize = 32;
pub(crate) const HEADER_OFFSET_SPEC_ARRAYREFS: usize = 40;
pub(crate) const HEADER_LEN_SPEC_ARRAYREFS: usize = 48;
pub(crate) const HEADER_OFFSET_CHROM_ENTRIES: usize = 56;
pub(crate) const HEADER_LEN_CHROM_ENTRIES: usize = 64;
pub(crate) const HEADER_OFFSET_CHROM_ARRAYREFS: usize = 72;
pub(crate) const HEADER_LEN_CHROM_ARRAYREFS: usize = 80;
pub(crate) const HEADER_OFFSET_SPEC_META: usize = 88;
pub(crate) const HEADER_LEN_SPEC_META: usize = 96;
pub(crate) const HEADER_OFFSET_CHROM_META: usize = 104;
pub(crate) const HEADER_LEN_CHROM_META: usize = 112;
pub(crate) const HEADER_OFFSET_GLOBAL_META: usize = 120;
pub(crate) const HEADER_LEN_GLOBAL_META: usize = 128;
pub(crate) const HEADER_OFFSET_PACKED_SPECTRA: usize = 136;
pub(crate) const HEADER_LEN_PACKED_SPECTRA: usize = 144;
pub(crate) const HEADER_OFFSET_PACKED_CHROMS: usize = 152;
pub(crate) const HEADER_LEN_PACKED_CHROMS: usize = 160;
pub(crate) const HEADER_SPECTRUM_BLOCK_COUNT: usize = 168;
pub(crate) const HEADER_CHROM_BLOCK_COUNT: usize = 176;
pub(crate) const HEADER_SPECTRUM_COUNT: usize = 184;
pub(crate) const HEADER_CHROM_COUNT: usize = 192;
pub(crate) const HEADER_SPEC_META_ROW_COUNT: usize = 200;
pub(crate) const HEADER_SPEC_META_NUMERIC_COUNT: usize = 208;
pub(crate) const HEADER_SPEC_META_STRING_COUNT: usize = 216;
pub(crate) const HEADER_CHROM_META_ROW_COUNT: usize = 224;
pub(crate) const HEADER_CHROM_META_NUMERIC_COUNT: usize = 232;
pub(crate) const HEADER_CHROM_META_STRING_COUNT: usize = 240;
pub(crate) const HEADER_GLOBAL_META_ROW_COUNT: usize = 248;
pub(crate) const HEADER_GLOBAL_META_NUMERIC_COUNT: usize = 256;
pub(crate) const HEADER_GLOBAL_META_STRING_COUNT: usize = 264;
pub(crate) const HEADER_SPEC_ARRAY_TYPE_COUNT: usize = 272;
pub(crate) const HEADER_CHROM_ARRAY_TYPE_COUNT: usize = 280;
pub(crate) const HEADER_SPEC_META_UNCOMPRESSED_SIZE: usize = 288;
pub(crate) const HEADER_CHROM_META_UNCOMPRESSED_SIZE: usize = 296;
pub(crate) const HEADER_GLOBAL_META_UNCOMPRESSED_SIZE: usize = 304;
pub(crate) const HEADER_OFF_FILTER_INDEX: usize = 312;
pub(crate) const HEADER_LEN_FILTER_INDEX: usize = 320;
pub(crate) const HEADER_TOTAL_FILE_SIZE: usize = 328;
pub(crate) const HEADER_SPEC_META_CRC32: usize = 1008;
pub(crate) const HEADER_CHROM_META_CRC32: usize = 1012;
pub(crate) const HEADER_GLOBAL_META_CRC32: usize = 1016;
pub(crate) const HEADER_CRC32: usize = 1020;

pub(crate) fn validate_file_integrity(bytes: &[u8], h: &Header) -> (bool, Vec<String>) {
    let mut failures = Vec::new();
    let file_len = bytes.len() as u64;

    let computed_header_crc = crc32fast::hash(&bytes[0..1020]);
    if computed_header_crc != h.header_crc32 {
        failures.push(format!(
            "condition 2: header_crc32 mismatch (stored={:#010x}, computed={:#010x})",
            h.header_crc32, computed_header_crc
        ));
    }

    if bytes.len() < 8 || &bytes[bytes.len() - 8..] != b"END\0\0\0\0\0" {
        failures.push("condition 3: missing or invalid file trailer".to_string());
    }

    if file_len != h.total_file_size {
        failures.push(format!(
            "condition 4: file size mismatch (actual={file_len}, expected={})",
            h.total_file_size
        ));
    }

    if h.spectrum_count * 16 != h.len_spec_entries {
        failures.push(format!(
            "condition 5: spectrum_count × 16 ({}) != len_spec_entries ({})",
            h.spectrum_count * 16,
            h.len_spec_entries
        ));
    }

    if h.chrom_count * 16 != h.len_chrom_entries {
        failures.push(format!(
            "condition 6: chrom_count × 16 ({}) != len_chrom_entries ({})",
            h.chrom_count * 16,
            h.len_chrom_entries
        ));
    }

    if h.len_spec_arrayrefs % 32 != 0 {
        failures.push(format!(
            "condition 7: len_spec_arrayrefs ({}) is not a multiple of 32",
            h.len_spec_arrayrefs
        ));
    }

    if h.len_chrom_arrayrefs % 32 != 0 {
        failures.push(format!(
            "condition 8: len_chrom_arrayrefs ({}) is not a multiple of 32",
            h.len_chrom_arrayrefs
        ));
    }

    if h.block_count_spect * 32 > h.len_container_spect {
        failures.push(format!(
            "condition 9: block_count_spect × 32 ({}) > len_container_spect ({})",
            h.block_count_spect * 32,
            h.len_container_spect
        ));
    }

    if h.block_count_chrom * 32 > h.len_container_chrom {
        failures.push(format!(
            "condition 10: block_count_chrom × 32 ({}) > len_container_chrom ({})",
            h.block_count_chrom * 32,
            h.len_container_chrom
        ));
    }

    if h.len_filter_index != h.spectrum_count * FILTER_INDEX_RECORD_SIZE as u64 {
        failures.push(format!(
            "condition 11: len_filter_index ({}) != spectrum_count × 48 ({})",
            h.len_filter_index,
            h.spectrum_count * 48
        ));
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
            h.off_container_spect,
            h.len_container_spect,
        ),
        (
            "container_chrom",
            h.off_container_chrom,
            h.len_container_chrom,
        ),
        ("filter_index", h.off_filter_index, h.len_filter_index),
    ];

    for &(name, off, len) in sections {
        match off.checked_add(len) {
            None => failures.push(format!("condition 12: {name} offset+length overflows u64")),
            Some(end) if end > trailer_start => failures.push(format!(
                "condition 12: {name} end ({end}) exceeds trailer_start ({trailer_start})"
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
        if prev_off + prev_len > curr_off {
            failures.push(format!(
                "condition 13: sections {prev_name} and {curr_name} overlap \
                 ({prev_name} ends at {}, {curr_name} starts at {curr_off})",
                prev_off + prev_len
            ));
        }
    }

    for &(name, off, len) in sections {
        if len > 0 && off % 8 != 0 {
            failures.push(format!(
                "condition 14: {name} offset ({off}) is not 8-byte aligned"
            ));
        }
    }

    for (name, off, len, stored) in [
        (
            "spec_meta",
            h.off_spec_meta,
            h.len_spec_meta,
            h.spec_meta_crc32,
        ),
        (
            "chrom_meta",
            h.off_chrom_meta,
            h.len_chrom_meta,
            h.chrom_meta_crc32,
        ),
        (
            "global_meta",
            h.off_global_meta,
            h.len_global_meta,
            h.global_meta_crc32,
        ),
    ] {
        match bytes.get(off as usize..(off + len) as usize) {
            None => failures.push(format!("meta crc32: {name} section out of bounds")),
            Some(section) => {
                let computed = crc32fast::hash(section);
                if computed != stored {
                    failures.push(format!(
                        "meta crc32: {name} mismatch (stored={:#010x}, computed={:#010x})",
                        stored, computed
                    ));
                }
            }
        }
    }

    let passed = failures.is_empty();
    (passed, failures)
}
