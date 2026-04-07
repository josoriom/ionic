mod common;

use common::assertions::*;
use common::fixtures;

#[test]
fn all_refs_resolved_for_all_pwiz_fixtures() {
    for rel in common::fixtures::PWIZ_FIXTURES {
        let mzml = common::parse_fixture(rel);
        assert_all_refs_resolved(&mzml);
    }
}

#[test]
#[should_panic(expected = "unresolved")]
fn detects_broken_run_default_source_file_ref() {
    let mut m = fixtures::tiny_pwiz_11().clone();
    m.run.default_source_file_ref = Some("missing-source-file-id".to_string());
    assert_all_refs_resolved(&m);
}

#[test]
#[should_panic(expected = "unresolved")]
fn detects_broken_precursor_spectrum_ref() {
    let mut m = fixtures::tiny_pwiz_11().clone();
    let s20 = m
        .run
        .spectrum_list
        .as_mut()
        .expect("spectrumList")
        .spectra
        .iter_mut()
        .find(|s| s.id == "scan=20")
        .expect("scan=20");
    if let Some(pl) = s20.precursor_list.as_mut() {
        pl.precursors[0].spectrum_ref = Some("scan=DOES_NOT_EXIST".to_string());
    }
    assert_all_refs_resolved(&m);
}
