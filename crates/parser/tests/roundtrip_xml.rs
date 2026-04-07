#[macro_use]
mod common;

use common::assertions::*;
use common::fixtures;
use ionic::mzml::bin_to_mzml::bin_to_mzml;

// Tests 1-7 from plan + Test 8 (small_zlib)

roundtrip_xml!(tiny_10_semantic, fixtures::tiny_pwiz_10);
roundtrip_xml!(tiny_11_semantic, fixtures::tiny_pwiz_11);
roundtrip_xml!(small_10_structural, fixtures::small_pwiz_10, structural);
roundtrip_xml!(small_11_structural, fixtures::small_pwiz_11, structural);
roundtrip_xml!(tiny_111_semantic, fixtures::tiny_pwiz_111);
roundtrip_xml!(small_miape_11_semantic, fixtures::small_miape_pwiz_11);
roundtrip_xml!(small_zlib_11_semantic, fixtures::small_zlib_pwiz_11);

#[test]
fn tiny2_exposes_known_unresolved_precursor_ref() {
    let mzml = fixtures::tiny2_pwiz_10();
    let precursor = common::precursor_list_of_spectrum(common::spectrum_by_id(mzml, "S2"))
        .and_then(|list| list.precursors.first())
        .expect("tiny2 fixture should retain its known precursor reference");
    assert_eq!(precursor.spectrum_ref.as_deref(), Some("change_me"));
}

#[test]
fn small_zlib_decodes_nonempty_compressed_arrays() {
    use common::BinaryDataExt;
    let mzml = fixtures::small_zlib_pwiz_11();
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
    // Parse -> serialize -> reparse -> serialize -> reparse -> eq
    let src = fixtures::tiny_pwiz_11();
    let xml = bin_to_mzml(src).expect("bin_to_mzml should succeed");
    let reparsed_once = common::parse_xml(&xml);
    let xml2 = bin_to_mzml(&reparsed_once).expect("second bin_to_mzml should succeed");
    let reparsed_twice = common::parse_xml(&xml2);
    assert_mzml_semantic_eq(&reparsed_once, &reparsed_twice);
}

#[test]
fn all_fixtures_can_parse() {
    for rel in common::fixtures::ALL_FIXTURES {
        let mzml = common::parse_fixture(rel);
        // Just verify it parsed without panic
        let _ = common::spectra(&mzml);
    }
}
