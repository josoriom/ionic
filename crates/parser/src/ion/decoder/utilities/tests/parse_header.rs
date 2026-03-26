use crate::ion::utilities::parse_header;
use std::{fs, path::PathBuf};

const PATH: &str = "data/ion/test.ion";

fn read_bytes(path: &str) -> Vec<u8> {
    let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read(&full).unwrap_or_else(|e| panic!("cannot read {:?}: {}", full, e))
}

#[test]
fn check_header() {
    let bytes = read_bytes(PATH);
    let header = parse_header(&bytes).expect("parse_header failed");

    assert_eq!(header.file_signature, *b"START\0\0\0");
    assert_eq!(header.endianness_flag, 0);
    assert_eq!(header.spectrum_count, 2);
    assert_eq!(header.chrom_count, 2);
    assert_eq!(header.spec_meta_count, 71);
    assert_eq!(header.spec_meta_num_count, 34);
    assert_eq!(header.spec_meta_str_count, 3);
    assert_eq!(header.chrom_meta_count, 45);
    assert_eq!(header.chrom_meta_num_count, 15);
    assert_eq!(header.chrom_meta_str_count, 5);
    assert_eq!(header.global_meta_count, 70);
    assert_eq!(header.global_meta_num_count, 8);
    assert_eq!(header.global_meta_str_count, 26);
    assert_eq!(header.block_count_spect, 2);
    assert_eq!(header.block_count_chrom, 2);
    assert_eq!(header.compression_codec, 1);
    assert_eq!(header.compression_level, 22);
    assert_eq!(header.array_filter, 1);
    assert!(header.spect_array_count > 0);
    assert!(header.chrom_array_count > 0);
    assert_eq!(header.target_block_uncompressed_bytes, 64 * 1024 * 1024);
    assert!(header.spec_meta_uncompressed_bytes > 0);
    assert!(header.chrom_meta_uncompressed_bytes > 0);
    assert!(header.global_meta_uncompressed_bytes > 0);
    assert_eq!(header.len_filter_index, header.spectrum_count * 48);

    for &off in &[
        header.off_spec_entries,
        header.off_spec_arrayrefs,
        header.off_chrom_entries,
        header.off_chrom_arrayrefs,
        header.off_spec_meta,
        header.off_chrom_meta,
        header.off_global_meta,
        header.off_container_spect,
        header.off_container_chrom,
        header.off_filter_index,
    ] {
        assert!(off >= 1024, "offset {off} must be >= 1024");
        assert_eq!(off % 8, 0, "offset {off} must be 8-aligned");
    }

    assert!(
        header
            .off_spec_entries
            .checked_add(header.len_spec_entries)
            .unwrap_or(u64::MAX)
            <= header.off_spec_arrayrefs
    );
    assert!(
        header
            .off_spec_arrayrefs
            .checked_add(header.len_spec_arrayrefs)
            .unwrap_or(u64::MAX)
            <= header.off_chrom_entries
    );
    assert!(
        header
            .off_chrom_entries
            .checked_add(header.len_chrom_entries)
            .unwrap_or(u64::MAX)
            <= header.off_chrom_arrayrefs
    );
    assert!(
        header
            .off_chrom_arrayrefs
            .checked_add(header.len_chrom_arrayrefs)
            .unwrap_or(u64::MAX)
            <= header.off_spec_meta
    );
    assert!(
        header
            .off_spec_meta
            .checked_add(header.len_spec_meta)
            .unwrap_or(u64::MAX)
            <= header.off_chrom_meta
    );
    assert!(
        header
            .off_chrom_meta
            .checked_add(header.len_chrom_meta)
            .unwrap_or(u64::MAX)
            <= header.off_global_meta
    );
    assert!(
        header
            .off_container_chrom
            .checked_add(header.len_container_chrom)
            .unwrap_or(u64::MAX)
            <= header.off_spec_entries
    );
    assert!(
        header
            .off_container_spect
            .checked_add(header.len_container_spect)
            .unwrap_or(u64::MAX)
            <= header.off_container_chrom
    );
    assert!(header.len_container_spect >= header.block_count_spect * 32);
    assert!(header.len_container_chrom >= header.block_count_chrom * 32);

    assert_eq!(&bytes[5..8], &[0u8; 3]);
    assert_eq!(&bytes[336..1008], &[0u8; 672]);

    let len = bytes.len() as u64;
    for &off in &[
        header.off_spec_entries,
        header.off_spec_arrayrefs,
        header.off_chrom_entries,
        header.off_chrom_arrayrefs,
        header.off_spec_meta,
        header.off_chrom_meta,
        header.off_global_meta,
        header.off_container_spect,
        header.off_container_chrom,
        header.off_filter_index,
    ] {
        assert!(off < len, "offset {off} out of bounds (len={len})");
    }

    let end = header
        .off_container_chrom
        .checked_add(header.len_container_chrom)
        .unwrap_or(u64::MAX);
    assert!(
        end <= len,
        "container_chrom end out of bounds (end={end}, len={len})"
    );
}
