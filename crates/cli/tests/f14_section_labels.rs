mod common;
use std::{
    fs::OpenOptions,
    io::{Seek, SeekFrom, Write},
};

use common::*;

#[test]
fn check_output_structure_stable() {
    let output = ionic()
        .arg("cat")
        .arg("--check")
        .arg(ion_fixture("tiny.msdata.mzML0.99.9.ion"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    for expected in [
        "File Summary",
        "format_version",
        "Sections",
        "all 8 sections opened",
        "Integrity Checks",
        "23/23 checks passed",
    ] {
        assert!(
            stdout.contains(expected),
            "expected stdout to contain '{expected}', got: {stdout}"
        );
    }
}

#[test]
fn section_failure_label_preserved() {
    let dir = tempfile::TempDir::new().unwrap();
    let corrupted = dir.path().join("corrupted.ion");
    read_bytes_to_tempfile_copy(&ion_fixture("tiny.msdata.mzML0.99.9.ion"), &corrupted);

    let mut file = OpenOptions::new().write(true).open(&corrupted).unwrap();
    file.seek(SeekFrom::Start(32)).unwrap();
    file.write_all(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F])
        .unwrap();
    file.flush().unwrap();
    drop(file);

    let output = ionic()
        .arg("cat")
        .arg("--check")
        .arg(&corrupted)
        .output()
        .unwrap();

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("A0 \u{2014} spectrum m/z-window directory"),
        "got: {stdout}"
    );
}
