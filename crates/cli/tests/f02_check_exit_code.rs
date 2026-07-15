mod common;
use std::{
    fs::OpenOptions,
    io::{Seek, SeekFrom, Write},
};

use common::*;

#[test]
fn valid_ion_check_exits_zero() {
    let output = ionic()
        .arg("cat")
        .arg("--check")
        .arg(ion_fixture("tiny.msdata.mzML0.99.9.ion"))
        .output()
        .unwrap();

    assert!(output.status.success());
}

#[test]
fn corrupted_ion_check_exits_nonzero() {
    let dir = tempfile::TempDir::new().unwrap();
    let corrupted = dir.path().join("corrupted.ion");
    read_bytes_to_tempfile_copy(&ion_fixture("tiny.msdata.mzML0.99.9.ion"), &corrupted);

    let mut file = OpenOptions::new().write(true).open(&corrupted).unwrap();
    file.seek(SeekFrom::Start(4)).unwrap();
    file.write_all(&[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();
    file.flush().unwrap();
    drop(file);

    let output = ionic()
        .arg("cat")
        .arg("--check")
        .arg(&corrupted)
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "check on a corrupted file should exit nonzero"
    );
}
