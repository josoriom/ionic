mod common;
use std::fs;

use common::*;

#[test]
fn check_rejects_unsupported_version() {
    let dir = tempfile::TempDir::new().unwrap();
    let unsupported = dir.path().join("unsupported_version.ion");

    let mut bytes = fs::read(ion_fixture("tiny.msdata.mzML0.99.9.ion")).unwrap();
    bytes[9..11].copy_from_slice(&u16::MAX.to_le_bytes());
    let header_crc = crc32fast::hash(&bytes[0..1020]);
    bytes[1020..1024].copy_from_slice(&header_crc.to_le_bytes());
    fs::write(&unsupported, &bytes).unwrap();

    let output = ionic()
        .arg("cat")
        .arg("--check")
        .arg(&unsupported)
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "cat --check must reject a file whose format_version is unsupported"
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("format version supported"),
        "expected the version-support check to be listed as failing, got: {stdout}"
    );
}
