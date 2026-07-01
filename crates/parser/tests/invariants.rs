mod common;

use common::assertions::*;
use common::test_files;
use common::{
    chromatogram_arrays, chromatograms, decode_ion, encode_to_ion, spectra, spectrum_arrays,
};

#[test]
fn default_array_length_matches_payload() {
    let mzml = test_files::tiny_pwiz_11();
    for s in spectra(mzml) {
        let arrays = spectrum_arrays(s);
        if arrays.is_empty() {
            continue;
        }
        let first_len = arrays
            .iter()
            .filter_map(|a| a.binary.as_ref())
            .map(|b| b.len())
            .next();
        if let (Some(expected), Some(al)) = (first_len, s.default_array_length) {
            assert_eq!(expected, al, "spectrum {} array length mismatch", s.id);
        }
    }
    for c in chromatograms(mzml) {
        let arrays = chromatogram_arrays(c);
        if arrays.is_empty() {
            continue;
        }
        let first_len = arrays
            .iter()
            .filter_map(|a| a.binary.as_ref())
            .map(|b| b.len())
            .next();
        if let (Some(expected), Some(al)) = (first_len, c.default_array_length) {
            assert_eq!(expected, al, "chromatogram {} array length mismatch", c.id);
        }
    }
}

#[test]
fn declared_counts_consistent_for_all_pwiz_test_files() {
    for rel in common::test_files::PWIZ_TEST_FILES {
        let mzml = common::parse_test_file(rel);
        assert_declared_counts_consistent(&mzml);
    }
}

#[test]
fn declared_counts_consistent_after_ion_roundtrip() {
    let out = decode_ion(&encode_to_ion(test_files::tiny_pwiz_11(), 12, false))
        .expect("decode should succeed");
    assert_declared_counts_consistent(&out);
}
