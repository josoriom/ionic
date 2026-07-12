use crate::{
    ion::{IonReader, IonResult, ReadOptions},
    mzml::structs::MzML,
};

const HEADER_SIZE: usize = 1024;
const OLD_ARRAY_ADDRESS_BYTES: usize = 40;
const NEW_ARRAY_ADDRESS_BYTES: usize = 32;
const DROPPED_PER_RECORD: usize = OLD_ARRAY_ADDRESS_BYTES - NEW_ARRAY_ADDRESS_BYTES;

const BLOCK_LAYOUT_BYTE: usize = 14;
const RESERVED_START: usize = 432;
const RESERVED_END: usize = 968;

const OFF_A0: usize = 32;
const OFF_A1: usize = 48;
const OFF_A2: usize = 64;
const LEN_A2: usize = 72;
const OFF_A3: usize = 80;
const LEN_A3: usize = 88;
const OFF_B0: usize = 96;
const OFF_B1: usize = 112;
const OFF_B2: usize = 128;
const LEN_B2: usize = 136;
const OFF_B3: usize = 144;
const LEN_B3: usize = 152;
const OFF_C: usize = 160;
const OFF_D: usize = 176;
const OFF_E: usize = 192;
const OFF_SPEC_CONTAINER: usize = 208;
const OFF_CHROM_CONTAINER: usize = 224;
const TOTAL_FILE_SIZE: usize = 400;
const A3_CRC32: usize = 980;
const B3_CRC32: usize = 996;
const HEADER_CRC32: usize = 1020;

const SECTION_OFFSET_FIELDS: [usize; 13] = [
    OFF_A0,
    OFF_A1,
    OFF_A2,
    OFF_A3,
    OFF_B0,
    OFF_B1,
    OFF_B2,
    OFF_B3,
    OFF_C,
    OFF_D,
    OFF_E,
    OFF_SPEC_CONTAINER,
    OFF_CHROM_CONTAINER,
];

const SWAP_U64_FIELDS: [(usize, usize); 4] = [(32, 48), (40, 56), (96, 112), (104, 120)];
const SWAP_U32_FIELDS: [(usize, usize); 2] = [(968, 972), (984, 988)];

pub fn read_old_file_to_mzml(bytes: &[u8]) -> IonResult<MzML> {
    let upgraded = upgrade_old_ion(bytes)?;
    let mut reader = IonReader::open(&upgraded, ReadOptions::default())?;
    reader.to_mzml()
}

pub fn upgrade_old_ion(old: &[u8]) -> IonResult<Vec<u8>> {
    if old.len() < HEADER_SIZE {
        return Err("legacy file is smaller than the header".into());
    }

    let off_a3 = read_u64(old, OFF_A3) as usize;
    let len_a3 = read_u64(old, LEN_A3) as usize;
    let off_b3 = read_u64(old, OFF_B3) as usize;
    let len_b3 = read_u64(old, LEN_B3) as usize;

    let spec_count = max_address_index(read_section(old, OFF_A2, LEN_A2)?) as usize;
    let chrom_count = max_address_index(read_section(old, OFF_B2, LEN_B2)?) as usize;

    match record_size(len_a3, spec_count)? {
        NEW_ARRAY_ADDRESS_BYTES => return sanitize_pre_release_fields(old),
        OLD_ARRAY_ADDRESS_BYTES => {}
        other => {
            return Err(format!(
                "legacy file has an unexpected array address record size ({other})"
            )
            .into());
        }
    }
    if len_b3 > 0 && record_size(len_b3, chrom_count)? != OLD_ARRAY_ADDRESS_BYTES {
        return Err("legacy file: chromatogram array records are not the expected size".into());
    }
    if len_b3 > 0 && off_a3 >= off_b3 {
        return Err("legacy file: unexpected physical order of the array address tables".into());
    }

    let d_spec = len_a3 / OLD_ARRAY_ADDRESS_BYTES * DROPPED_PER_RECORD;
    let d_chrom = len_b3 / OLD_ARRAY_ADDRESS_BYTES * DROPPED_PER_RECORD;

    let mut out = Vec::with_capacity(old.len() - d_spec - d_chrom);
    out.extend_from_slice(&old[..off_a3]);
    narrow_records_into(&mut out, &old[off_a3..off_a3 + len_a3]);
    if len_b3 > 0 {
        out.extend_from_slice(&old[off_a3 + len_a3..off_b3]);
        narrow_records_into(&mut out, &old[off_b3..off_b3 + len_b3]);
        out.extend_from_slice(&old[off_b3 + len_b3..]);
    } else {
        out.extend_from_slice(&old[off_a3 + len_a3..]);
    }

    for field in SECTION_OFFSET_FIELDS {
        let value = read_u64(&out, field) as usize;
        let mut shifted = value;
        if value > off_a3 {
            shifted -= d_spec;
        }
        if len_b3 > 0 && value > off_b3 {
            shifted -= d_chrom;
        }
        write_u64(&mut out, field, shifted as u64);
    }

    write_u64(
        &mut out,
        LEN_A3,
        (len_a3 / OLD_ARRAY_ADDRESS_BYTES * NEW_ARRAY_ADDRESS_BYTES) as u64,
    );
    write_u64(
        &mut out,
        LEN_B3,
        (len_b3 / OLD_ARRAY_ADDRESS_BYTES * NEW_ARRAY_ADDRESS_BYTES) as u64,
    );

    for (left, right) in SWAP_U64_FIELDS {
        let (a, b) = (read_u64(&out, left), read_u64(&out, right));
        write_u64(&mut out, left, b);
        write_u64(&mut out, right, a);
    }
    for (left, right) in SWAP_U32_FIELDS {
        let (a, b) = (read_u32(&out, left), read_u32(&out, right));
        write_u32(&mut out, left, b);
        write_u32(&mut out, right, a);
    }

    let new_off_a3 = read_u64(&out, OFF_A3) as usize;
    let new_len_a3 = read_u64(&out, LEN_A3) as usize;
    let a3_crc = crc32fast::hash(&out[new_off_a3..new_off_a3 + new_len_a3]);
    write_u32(&mut out, A3_CRC32, a3_crc);
    let new_len_b3 = read_u64(&out, LEN_B3) as usize;
    if new_len_b3 > 0 {
        let new_off_b3 = read_u64(&out, OFF_B3) as usize;
        let b3_crc = crc32fast::hash(&out[new_off_b3..new_off_b3 + new_len_b3]);
        write_u32(&mut out, B3_CRC32, b3_crc);
    }

    out[BLOCK_LAYOUT_BYTE] = 0;
    out[BLOCK_LAYOUT_BYTE + 1] = 0;
    for byte in &mut out[RESERVED_START..RESERVED_END] {
        *byte = 0;
    }
    let total = out.len() as u64;
    write_u64(&mut out, TOTAL_FILE_SIZE, total);
    let header_crc = crc32fast::hash(&out[0..HEADER_CRC32]);
    write_u32(&mut out, HEADER_CRC32, header_crc);
    Ok(out)
}

fn record_size(len: usize, count: usize) -> IonResult<usize> {
    if count == 0 {
        return Ok(NEW_ARRAY_ADDRESS_BYTES);
    }
    if !len.is_multiple_of(count) {
        return Err(
            "legacy file: array address length is not a multiple of the record count".into(),
        );
    }
    Ok(len / count)
}

fn narrow_records_into(out: &mut Vec<u8>, section: &[u8]) {
    for record in section.chunks_exact(OLD_ARRAY_ADDRESS_BYTES) {
        out.extend_from_slice(&record[..NEW_ARRAY_ADDRESS_BYTES]);
    }
}

fn sanitize_pre_release_fields(old: &[u8]) -> IonResult<Vec<u8>> {
    let mut clean = old.to_vec();
    clean[BLOCK_LAYOUT_BYTE] = 0;
    clean[BLOCK_LAYOUT_BYTE + 1] = 0;
    for byte in &mut clean[RESERVED_START..RESERVED_END] {
        *byte = 0;
    }
    let header_crc = crc32fast::hash(&clean[0..HEADER_CRC32]);
    write_u32(&mut clean, HEADER_CRC32, header_crc);
    Ok(clean)
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

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
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
        experimental[RESERVED_START..RESERVED_START + 8].copy_from_slice(&50.0f64.to_le_bytes());
        let header_crc = crc32fast::hash(&experimental[0..HEADER_CRC32]);
        experimental[HEADER_CRC32..HEADER_SIZE].copy_from_slice(&header_crc.to_le_bytes());

        let from_experimental = read_old_file_to_mzml(&experimental).unwrap();
        let from_clean = read_old_file_to_mzml(SAMPLE).unwrap();
        assert_eq!(format!("{from_experimental:?}"), format!("{from_clean:?}"));
    }
}
