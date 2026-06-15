use crate::ion::{IonReader, IonResult, ReadOptions};
use crate::mzml::structs::MzML;

const FILE_HEADER_SIZE: usize = 1024;
const BLOCK_LAYOUT_BYTE: usize = 14;
const MZ_WINDOW_START: usize = 432;
const MZ_WINDOW_END: usize = 440;
const HEADER_CRC_START: usize = 1020;

pub fn read_old_file_to_mzml(bytes: &[u8]) -> IonResult<MzML> {
    let clean = clear_pre_release_window_fields(bytes)?;
    let mut reader = IonReader::open(&clean, ReadOptions::default())?;
    reader.to_mzml()
}

fn clear_pre_release_window_fields(bytes: &[u8]) -> IonResult<Vec<u8>> {
    if bytes.len() < FILE_HEADER_SIZE {
        return Err("legacy file is smaller than the header".into());
    }
    let mut clean = bytes.to_vec();
    clean[BLOCK_LAYOUT_BYTE] = 0;
    clean[BLOCK_LAYOUT_BYTE + 1] = 0;
    for byte in &mut clean[MZ_WINDOW_START..MZ_WINDOW_END] {
        *byte = 0;
    }
    let header_crc = crc32fast::hash(&clean[0..HEADER_CRC_START]);
    clean[HEADER_CRC_START..FILE_HEADER_SIZE].copy_from_slice(&header_crc.to_le_bytes());
    Ok(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = include_bytes!("../../data/ion/test.ion");

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
        let header_crc = crc32fast::hash(&experimental[0..HEADER_CRC_START]);
        experimental[HEADER_CRC_START..FILE_HEADER_SIZE].copy_from_slice(&header_crc.to_le_bytes());

        let from_experimental = read_old_file_to_mzml(&experimental).unwrap();
        let from_clean = read_old_file_to_mzml(SAMPLE).unwrap();
        assert_eq!(format!("{from_experimental:?}"), format!("{from_clean:?}"));
    }
}
