mod common;
use common::*;

#[test]
fn many_files_still_converts() {
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
        .arg("--many-files")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output_dir
            .path()
            .join("tiny.msdata.mzML0.99.9.ion")
            .exists()
    );
}
