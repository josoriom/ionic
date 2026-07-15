mod common;
use common::*;

#[test]
fn no_color_env_suppresses_ansi_on_check() {
    let output = ionic()
        .arg("cat")
        .arg("--check")
        .arg(ion_fixture("tiny.msdata.mzML0.99.9.ion"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains('\x1b'),
        "stdout should have no ANSI escapes under NO_COLOR, got: {stdout}"
    );
    assert!(
        !stderr.contains('\x1b'),
        "stderr should have no ANSI escapes under NO_COLOR, got: {stderr}"
    );
}

#[test]
fn no_color_env_suppresses_ansi_on_convert() {
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
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains('\x1b'),
        "stdout should have no ANSI escapes under NO_COLOR, got: {stdout}"
    );
}
