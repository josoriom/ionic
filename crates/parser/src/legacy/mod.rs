use crate::ion::{IonReader, IonResult, ReadOptions};
use crate::mzml::structs::MzML;

const HEADER_SIZE: usize = 1024;
const ARRAY_ADDRESS_BYTES: usize = 32;

const BLOCK_LAYOUT_BYTE: usize = 14;
const MZ_WINDOW_START: usize = 432;
const MZ_WINDOW_END: usize = 440;

const OFF_A2: usize = 64;
const LEN_A2: usize = 72;
const LEN_A3: usize = 88;
const OFF_B2: usize = 128;
const LEN_B2: usize = 136;
const LEN_B3: usize = 152;
const HEADER_CRC32: usize = 1020;

pub fn read_old_file_to_mzml(bytes: &[u8]) -> IonResult<MzML> {
    let upgraded = upgrade_old_ion(bytes)?;
    let mut reader = IonReader::open(&upgraded, ReadOptions::default())?;
    reader.to_mzml()
}

pub fn upgrade_old_ion(old: &[u8]) -> IonResult<Vec<u8>> {
    if old.len() < HEADER_SIZE {
        return Err("legacy file is smaller than the header".into());
    }

    let len_a3 = read_u64(old, LEN_A3) as usize;
    let len_b3 = read_u64(old, LEN_B3) as usize;
    let spec_count = max_address_index(read_section(old, OFF_A2, LEN_A2)?);
    let chrom_count = max_address_index(read_section(old, OFF_B2, LEN_B2)?);

    ensure_current_record_size(len_a3, spec_count)?;
    ensure_current_record_size(len_b3, chrom_count)?;

    sanitize_pre_release_fields(old)
}

fn sanitize_pre_release_fields(old: &[u8]) -> IonResult<Vec<u8>> {
    let mut clean = old.to_vec();
    clean[BLOCK_LAYOUT_BYTE] = 0;
    clean[BLOCK_LAYOUT_BYTE + 1] = 0;
    for byte in &mut clean[MZ_WINDOW_START..MZ_WINDOW_END] {
        *byte = 0;
    }
    let header_crc = crc32fast::hash(&clean[0..HEADER_CRC32]);
    write_u32(&mut clean, HEADER_CRC32, header_crc);
    Ok(clean)
}

fn ensure_current_record_size(len: usize, count: u64) -> IonResult<()> {
    if count == 0 {
        return Ok(());
    }
    let count = count as usize;
    if !len.is_multiple_of(count) {
        return Err("legacy file: array address length is not a multiple of the record count".into());
    }
    if len / count != ARRAY_ADDRESS_BYTES {
        return Err(
            "legacy file uses a non-current array address record size; re-encode it from mzML".into(),
        );
    }
    Ok(())
}

fn max_address_index(entries: &[u8]) -> u64 {
    let mut total = 0u64;
    for entry in entries.chunks_exact(16) {
        let first = read_u64(entry, 0);
        let count = read_u64(entry, 8);
        total = total.max(first.saturating_add(count));
    }
    total
}

fn read_section(bytes: &[u8], off_at: usize, len_at: usize) -> IonResult<&[u8]> {
    let off = read_u64(bytes, off_at) as usize;
    let len = read_u64(bytes, len_at) as usize;
    bytes
        .get(off..off + len)
        .ok_or_else(|| "legacy file: section out of bounds".into())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = include_bytes!("old_test.ion");

    #[test]
    fn reads_a_clean_file() {
        let mzml = read_old_file_to_mzml(SAMPLE).unwrap();
        assert!(mzml.run.spectrum_list.is_some());
    }

    #[test]
    fn reads_a_file_that_still_has_the_pre_release_window_fields() {
        let mut experimental = SAMPLE.to_vec();
        experimental[BLOCK_LAYOUT_BYTE] = 1;
        experimental[MZ_WINDOW_START..MZ_WINDOW_END].copy_from_slice(&50.0f64.to_le_bytes());
        let header_crc = crc32fast::hash(&experimental[0..HEADER_CRC32]);
        experimental[HEADER_CRC32..HEADER_SIZE].copy_from_slice(&header_crc.to_le_bytes());

        let from_experimental = read_old_file_to_mzml(&experimental).unwrap();
        let from_clean = read_old_file_to_mzml(SAMPLE).unwrap();
        assert_eq!(format!("{from_experimental:?}"), format!("{from_clean:?}"));
    }
}
