mod common;
use common::*;

#[test]
fn check_valid_ion_reports_all_passed() {
    let output = ionic()
        .arg("cat")
        .arg("--check")
        .arg(ion_fixture("tiny.msdata.mzML0.99.9.ion"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("23/23 checks passed"), "got: {stdout}");
}

#[test]
fn cat_ion_metadata_is_json() {
    let output = ionic()
        .arg("cat")
        .arg(ion_fixture("tiny.msdata.mzML0.99.9.ion"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.trim().starts_with('{'), "got: {stdout}");
}
