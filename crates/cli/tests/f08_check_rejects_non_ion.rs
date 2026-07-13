mod common;
use common::*;

#[test]
fn check_on_mzml_rejected() {
    let output = ionic()
        .arg("cat")
        .arg("--check")
        .arg(mzml_fixture("tiny.msdata.mzML0.99.9.mzML"))
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "--check on a non-.ion file should fail"
    );
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        stderr.contains(".ion"),
        "stderr should mention .ion, got: {stderr}"
    );
}
