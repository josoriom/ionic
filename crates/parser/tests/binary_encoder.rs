mod common;

use common::test_files;
use common::{decode_ion, encode_to_ion, top_level_software_ids, top_level_source_file_ids};

#[test]
fn ion_header_signature_is_correct() {
    let bytes = encode_to_ion(test_files::tiny_pwiz_11(), 12, false);
    assert!(
        bytes.len() > 1024,
        "encoded bytes should include header and payload"
    );
    assert_eq!(&bytes[..8], b"START\0\0\0");
}

#[test]
fn deterministic_for_same_input_and_config() {
    let src = test_files::tiny_pwiz_11();
    let a = encode_to_ion(src, 9, false);
    let b = encode_to_ion(src, 9, false);
    assert_eq!(a, b, "encode must be deterministic for same input/config");
}

#[test]
fn ion_roundtrip_preserves_source_file_ids() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 12, false);
    let out = decode_ion(&bytes).expect("decode should succeed");
    assert_eq!(
        top_level_source_file_ids(src),
        top_level_source_file_ids(&out),
        "source file ids changed after ion roundtrip"
    );
}

#[test]
fn ion_roundtrip_preserves_software_ids() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 12, false);
    let out = decode_ion(&bytes).expect("decode should succeed");
    assert_eq!(
        top_level_software_ids(src),
        top_level_software_ids(&out),
        "software ids changed after ion roundtrip"
    );
}
