mod common;
use common::*;

fn input_dir_with_one_mzml() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    read_bytes_to_tempfile_copy(
        &mzml_fixture("tiny.msdata.mzML0.99.9.mzML"),
        &dir.path().join("tiny.msdata.mzML0.99.9.mzML"),
    );
    dir
}

#[test]
fn mz_window_zero_rejected() {
    let input_dir = input_dir_with_one_mzml();
    let output_dir = tempfile::TempDir::new().unwrap();

    let output = ionic()
        .arg("convert")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .arg("--mz-window=0")
        .output()
        .unwrap();

    assert!(!output.status.success(), "--mz-window=0 should be rejected");
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        stderr.contains("mz-window"),
        "stderr should mention mz-window, got: {stderr}"
    );
}

#[test]
fn mz_window_negative_rejected() {
    let input_dir = input_dir_with_one_mzml();
    let output_dir = tempfile::TempDir::new().unwrap();

    let output = ionic()
        .arg("convert")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .arg("--mz-window=-5")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "--mz-window=-5 should be rejected"
    );
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        stderr.contains("mz-window"),
        "stderr should mention mz-window, got: {stderr}"
    );
}
