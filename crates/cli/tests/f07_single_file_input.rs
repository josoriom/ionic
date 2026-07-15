mod common;
use common::*;

#[test]
fn single_mzml_file_converts() {
    let input_dir = tempfile::TempDir::new().unwrap();
    let input_file = input_dir.path().join("tiny.msdata.mzML0.99.9.mzML");
    read_bytes_to_tempfile_copy(&mzml_fixture("tiny.msdata.mzML0.99.9.mzML"), &input_file);
    let output_dir = tempfile::TempDir::new().unwrap();

    let output = ionic()
        .arg("convert")
        .arg("-i")
        .arg(&input_file)
        .arg("-o")
        .arg(output_dir.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "converting a single file input should work, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output_dir
            .path()
            .join("tiny.msdata.mzML0.99.9.ion")
            .exists()
    );
}
