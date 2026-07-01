mod common;

use common::test_files;
use common::{decode_ion, encode_to_ion};
use ionic::ion::{HEADER_FORMAT_VERSION_OFFSET, format::MAX_SUPPORTED_VERSION};

#[test]
fn rejects_invalid_signature() {
    let mut bytes = encode_to_ion(test_files::tiny_pwiz_11(), 9, false);
    bytes[0] = b'X';
    let err = decode_ion(&bytes).expect_err("decode must reject invalid signature");
    assert!(err.contains("file_signature") || err.contains("signature"));
}

#[test]
fn rejects_unsupported_format_version() {
    let mut bytes = encode_to_ion(test_files::tiny_pwiz_11(), 9, false);
    let unsupported = MAX_SUPPORTED_VERSION + 1;
    bytes[HEADER_FORMAT_VERSION_OFFSET..HEADER_FORMAT_VERSION_OFFSET + 2]
        .copy_from_slice(&unsupported.to_le_bytes());
    let err = decode_ion(&bytes).expect_err("decode must reject unsupported format version");
    assert!(
        err.contains("unsupported format version"),
        "unexpected decode error: {err}"
    );
}

#[test]
fn accepts_all_supported_format_versions() {
    use ionic::ion::format::{MIN_SUPPORTED_VERSION, allow_version};
    for version in MIN_SUPPORTED_VERSION..=MAX_SUPPORTED_VERSION {
        assert!(
            allow_version(version).is_ok(),
            "version {version} must be allowed"
        );
    }
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
