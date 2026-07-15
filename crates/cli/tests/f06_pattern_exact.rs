mod common;
use common::*;

fn input_dir_with_both_mzmls() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    read_bytes_to_tempfile_copy(
        &mzml_fixture("tiny.msdata.mzML0.99.9.mzML"),
        &dir.path().join("tiny.msdata.mzML0.99.9.mzML"),
    );
    read_bytes_to_tempfile_copy(
        &mzml_fixture("tiny.msdata.mzML0.99.10.mzML"),
        &dir.path().join("tiny.msdata.mzML0.99.10.mzML"),
    );
    dir
}

#[test]
fn pattern_exact_rejects_substring() {
    let input_dir = input_dir_with_both_mzmls();
    let output_dir = tempfile::TempDir::new().unwrap();

    let output = ionic()
        .arg("convert")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .arg("--pattern-exact")
        .arg("tiny.msdata")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "no file is named exactly 'tiny.msdata', so nothing should match"
    );
}

#[test]
fn pattern_exact_matches_full_filename() {
    let input_dir = input_dir_with_both_mzmls();
    let output_dir = tempfile::TempDir::new().unwrap();

    let output = ionic()
        .arg("convert")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .arg("--pattern-exact")
        .arg("tiny.msdata.mzML0.99.9.mzML")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("ok=1"),
        "exactly one file should have matched, got: {stdout}"
    );
}
