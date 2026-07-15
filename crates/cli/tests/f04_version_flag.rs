mod common;
use common::*;

#[test]
fn root_version_prints() {
    let output = ionic().arg("-v").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().contains(env!("CARGO_PKG_VERSION")),
        "stdout should contain the crate version, got: {stdout}"
    );
}

#[test]
fn cat_v_does_not_silently_skip() {
    let output = ionic()
        .arg("cat")
        .arg("-v")
        .arg(ion_fixture("tiny.msdata.mzML0.99.9.ion"))
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "`cat -v` should not silently print the version and skip the command"
    );
}
