use std::{fs, path::PathBuf};

use crate::ion::{
    DecompressionBudget,
    decoder::decode::Metadatum,
    meta_groups::MetaTotals,
    utilities::{MetaGroupReader, parse_header},
};

fn read_file(path: &str) -> Vec<u8> {
    let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read(&full).unwrap_or_else(|error| panic!("cannot read {full:?}: {error}"))
}

pub(super) fn spectra_metadata(path: &str) -> Vec<Metadatum> {
    let bytes = read_file(path);
    let header = parse_header(&bytes).expect("parse_header failed");
    let start = header.off_spec_meta as usize;
    let end = start + header.len_spec_meta as usize;
    MetaGroupReader::new(
        &bytes[start..end],
        header.spec_meta_group_count,
        header.meta_group_size,
        header.spectrum_count,
        MetaTotals {
            rows: header.spec_meta_count,
            numeric: header.spec_meta_numeric_count,
            string: header.spec_meta_string_count,
            uncompressed: header.spec_meta_uncompressed_bytes,
        },
        header.compression_codec,
        true,
        DecompressionBudget::default(),
        64 * 1024 * 1024,
    )
    .expect("build spectra metadata reader")
    .read_all()
    .expect("read spectra metadata")
}

pub(super) fn chromatograms_metadata(path: &str) -> Vec<Metadatum> {
    let bytes = read_file(path);
    let header = parse_header(&bytes).expect("parse_header failed");
    let start = header.off_chrom_meta as usize;
    let end = start + header.len_chrom_meta as usize;
    MetaGroupReader::new(
        &bytes[start..end],
        header.chrom_meta_group_count,
        header.meta_group_size,
        header.chrom_count,
        MetaTotals {
            rows: header.chrom_meta_count,
            numeric: header.chrom_meta_numeric_count,
            string: header.chrom_meta_string_count,
            uncompressed: header.chrom_meta_uncompressed_bytes,
        },
        header.compression_codec,
        true,
        DecompressionBudget::default(),
        64 * 1024 * 1024,
    )
    .expect("build chromatograms metadata reader")
    .read_all()
    .expect("read chromatograms metadata")
}
