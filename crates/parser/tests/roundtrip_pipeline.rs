mod common;

use common::{assertions::*, decode_ion, encode_to_ion, semantic_fingerprint, test_files};
use ionic::mzml::{bin_to_mzml::bin_to_mzml, parse_mzml::parse_mzml};

#[test]
fn full_pipeline_tiny11() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 12, false);
    let out = decode_ion(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(src, &out);
}

#[test]
fn full_pipeline_tiny10() {
    let src = test_files::tiny_pwiz_10();
    let bytes = encode_to_ion(src, 7, false);
    let out = decode_ion(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(src, &out);
}

#[test]
fn mixed_pipeline_xml_then_ion() {
    let src = test_files::tiny_pwiz_11();
    let xml = bin_to_mzml(src).expect("bin_to_mzml should succeed");
    let reparsed = parse_mzml(&xml).expect("reparse should succeed");

    let bytes = encode_to_ion(&reparsed, 6, false);
    let decoded = decode_ion(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(&reparsed, &decoded);
}

#[test]
fn repeated_roundtrip_stability() {
    let src = test_files::tiny_pwiz_11();
    let mut last_fp = semantic_fingerprint(src);

    for iter in 0..25 {
        let bytes = encode_to_ion(src, 9, false);
        let out = decode_ion(&bytes).expect("decode should succeed");
        let fp = semantic_fingerprint(&out);
        assert_eq!(fp, last_fp, "semantic fingerprint changed at iter={iter}");
        last_fp = fp;
    }
}

#[test]
fn internal_test_file_regression_guard_099_10() {
    let src = common::parse_test_file("crates/parser/data/mzml/tiny.pwiz.mzML0.99.10.mzML");
    let bytes = encode_to_ion(&src, 12, false);
    let out = decode_ion(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(&src, &out);
}

#[test]
fn internal_test_file_regression_guard_anpc() {
    let src = test_files::anpc_test_mzml();
    let bytes = encode_to_ion(src, 12, false);
    let out = decode_ion(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(src, &out);
}
