#[macro_use]
mod common;

use common::assertions::*;
use common::test_files;
use ionic::mzml::{bin_to_mzml::bin_to_mzml, parse_mzml::parse_mzml};

roundtrip_xml!(tiny_10_semantic, test_files::tiny_pwiz_10);
roundtrip_xml!(tiny_11_semantic, test_files::tiny_pwiz_11);
roundtrip_xml!(small_10_structural, test_files::small_pwiz_10, structural);
roundtrip_xml!(small_11_structural, test_files::small_pwiz_11, structural);
roundtrip_xml!(tiny_111_semantic, test_files::tiny_pwiz_111);
roundtrip_xml!(small_miape_11_semantic, test_files::small_miape_pwiz_11);
roundtrip_xml!(small_zlib_11_semantic, test_files::small_zlib_pwiz_11);

#[test]
fn tiny2_exposes_known_unresolved_precursor_ref() {
    let mzml = test_files::tiny2_pwiz_10();
    let precursor = common::precursor_list_of_spectrum(common::spectrum_by_id(mzml, "S2"))
        .and_then(|list| list.precursors.first())
        .expect("tiny2 test_file should retain its known precursor reference");
    assert_eq!(precursor.spectrum_ref.as_deref(), Some("change_me"));
}

#[test]
fn small_zlib_decodes_nonempty_compressed_arrays() {
    use common::BinaryDataExt;
    let mzml = test_files::small_zlib_pwiz_11();
    let first_spectrum = common::spectra(mzml)
        .first()
        .expect("at least one spectrum");
    let arrays = common::spectrum_arrays(first_spectrum);
    assert!(
        arrays
            .iter()
            .any(|array| common::cv_has_accession(&array.cv_params, "MS:1000574")),
        "small_zlib should expose zlib compression metadata"
    );
    for (index, array) in arrays.iter().enumerate() {
        let binary = array
            .binary
            .as_ref()
            .unwrap_or_else(|| panic!("small_zlib spectrum array {index} missing decoded payload"));
        assert!(
            !binary.is_empty(),
            "small_zlib spectrum array {index} should decode to a non-empty payload"
        );
    }
}

#[test]
fn xml_roundtrip_idempotent() {
    let src = test_files::tiny_pwiz_11();
    let xml = bin_to_mzml(src).expect("bin_to_mzml should succeed");
    let reparsed_once = parse_mzml(&xml).expect("first reparse should succeed");
    let xml2 = bin_to_mzml(&reparsed_once).expect("second bin_to_mzml should succeed");
    let reparsed_twice = parse_mzml(&xml2).expect("second reparse should succeed");
    assert_mzml_semantic_eq(&reparsed_once, &reparsed_twice);
}

#[test]
fn all_test_files_can_parse() {
    for rel in common::test_files::ALL_TEST_FILES {
        let mzml = common::parse_test_file(rel);
        let _ = common::spectra(&mzml);
    }
}
