mod common;

use common::test_files;
use common::{decode_ion, encode_to_ion};

#[test]
fn rejects_invalid_signature() {
    let mut bytes = encode_to_ion(test_files::tiny_pwiz_11(), 9, false);
    bytes[0] = b'X';
    let err = decode_ion(&bytes).expect_err("decode must reject invalid signature");
    assert!(err.contains("file_signature") || err.contains("signature"));
}

#[test]
fn rejects_invalid_header_version_word() {
    let mut bytes = encode_to_ion(test_files::tiny_pwiz_11(), 9, false);
    bytes[4..8].copy_from_slice(b"X999");
    let err = decode_ion(&bytes).expect_err("decode must reject unsupported header version");
    assert!(
        err.contains("version") || err.contains("signature") || err.contains("endianness_flag"),
        "unexpected decode error: {err}"
    );
}

#[test]
fn rejects_truncated_payload() {
    let bytes = encode_to_ion(test_files::tiny_pwiz_11(), 9, false);
    let truncated = &bytes[..bytes.len() / 2];
    let _ = decode_ion(truncated).expect_err("decode must reject truncated payload");
}

#[test]
fn rejects_corrupted_offset_range() {
    let mut bytes = encode_to_ion(test_files::tiny_pwiz_11(), 9, false);
    bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    let _ = decode_ion(&bytes).expect_err("decode must reject invalid section offset");
}
