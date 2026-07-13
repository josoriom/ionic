mod common;
use common::*;

#[test]
fn benchmark_with_ion_to_mzml_conflicts() {
    let input_dir = tempfile::TempDir::new().unwrap();
    read_bytes_to_tempfile_copy(
        &ion_fixture("tiny.msdata.mzML0.99.9.ion"),
        &input_dir.path().join("tiny.msdata.mzML0.99.9.ion"),
    );
    let output_dir = tempfile::TempDir::new().unwrap();

    let output = ionic()
        .arg("convert")
        .arg("--ion-to-mzml")
        .arg("--benchmark-decode")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "combining --ion-to-mzml with --benchmark-decode should be rejected"
    );
}

#[test]
fn benchmark_decode_alone_still_works() {
    let input_dir = tempfile::TempDir::new().unwrap();
    read_bytes_to_tempfile_copy(
        &ion_fixture("tiny.msdata.mzML0.99.9.ion"),
        &input_dir.path().join("tiny.msdata.mzML0.99.9.ion"),
    );
    let output_dir = tempfile::TempDir::new().unwrap();

    let output = ionic()
        .arg("convert")
        .arg("--benchmark-decode")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
}
