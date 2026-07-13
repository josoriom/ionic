mod common;
use common::*;

#[test]
fn missing_output_path_reads_as_error_not_debug() {
    let input_dir = tempfile::TempDir::new().unwrap();
    read_bytes_to_tempfile_copy(
        &mzml_fixture("tiny.msdata.mzML0.99.9.mzML"),
        &input_dir.path().join("tiny.msdata.mzML0.99.9.mzML"),
    );

    let output = ionic()
        .arg("convert")
        .arg("-i")
        .arg(input_dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        !stderr.starts_with("Error: \""),
        "stderr should not read as a quoted Debug string, got: {stderr}"
    );
    assert!(
        stderr.contains("--output-path is required"),
        "stderr should mention the missing flag, got: {stderr}"
    );
}

#[test]
fn bad_regex_error_has_no_literal_backslash_n() {
    let input_dir = tempfile::TempDir::new().unwrap();
    read_bytes_to_tempfile_copy(
        &mzml_fixture("tiny.msdata.mzML0.99.9.mzML"),
        &input_dir.path().join("tiny.msdata.mzML0.99.9.mzML"),
    );
    let output_dir = tempfile::TempDir::new().unwrap();

    let output = ionic()
        .arg("convert")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .arg("--regex")
        .arg("(")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        !stderr.contains("\\n"),
        "stderr should not contain a literal backslash-n escape, got: {stderr}"
    );
}
