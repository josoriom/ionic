mod common;
use common::*;

fn tail_str(bytes: &[u8], max_len: usize) -> String {
    let start = bytes.len().saturating_sub(max_len);
    String::from_utf8_lossy(&bytes[start..]).trim().to_string()
}

#[test]
fn mzml_to_ion_then_rerun_skips() {
    let input_dir = tempfile::TempDir::new().unwrap();
    read_bytes_to_tempfile_copy(
        &mzml_fixture("tiny.msdata.mzML0.99.9.mzML"),
        &input_dir.path().join("tiny.msdata.mzML0.99.9.mzML"),
    );
    let output_dir = tempfile::TempDir::new().unwrap();

    let first = ionic()
        .arg("convert")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .output()
        .unwrap();
    assert!(first.status.success());
    let first_stdout = strip_ansi(&String::from_utf8_lossy(&first.stdout));
    assert!(first_stdout.contains("ok=1"), "got: {first_stdout}");

    let second = ionic()
        .arg("convert")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .output()
        .unwrap();
    assert!(second.status.success());
    let second_stdout = strip_ansi(&String::from_utf8_lossy(&second.stdout));
    assert!(second_stdout.contains("skipped=1"), "got: {second_stdout}");
}

#[test]
fn ion_to_mzml_produces_complete_output() {
    let input_dir = tempfile::TempDir::new().unwrap();
    read_bytes_to_tempfile_copy(
        &ion_fixture("tiny.msdata.mzML0.99.9.ion"),
        &input_dir.path().join("tiny.msdata.mzML0.99.9.ion"),
    );
    let output_dir = tempfile::TempDir::new().unwrap();

    let output = ionic()
        .arg("convert")
        .arg("--ion-to-mzml")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let produced = output_dir.path().join("tiny.msdata.mzML0.99.9.mzML");
    assert!(produced.exists());
    let bytes = std::fs::read(&produced).unwrap();
    assert!(
        tail_str(&bytes, 256).ends_with("</indexedmzML>"),
        "produced mzML should end with </indexedmzML>, tail: {}",
        tail_str(&bytes, 256)
    );
}

#[test]
fn truncated_mzml_output_is_regenerated_not_skipped() {
    let input_dir = tempfile::TempDir::new().unwrap();
    read_bytes_to_tempfile_copy(
        &ion_fixture("tiny.msdata.mzML0.99.9.ion"),
        &input_dir.path().join("tiny.msdata.mzML0.99.9.ion"),
    );
    let output_dir = tempfile::TempDir::new().unwrap();

    let first = ionic()
        .arg("convert")
        .arg("--ion-to-mzml")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .output()
        .unwrap();
    assert!(first.status.success());

    let produced = output_dir.path().join("tiny.msdata.mzML0.99.9.mzML");
    assert!(produced.exists());
    std::fs::write(&produced, b"<incomplete>").unwrap();

    let second = ionic()
        .arg("convert")
        .arg("--ion-to-mzml")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let bytes = std::fs::read(&produced).unwrap();
    assert!(
        tail_str(&bytes, 256).ends_with("</indexedmzML>"),
        "a truncated mzML output should be regenerated, not skipped, tail: {}",
        tail_str(&bytes, 256)
    );
}
