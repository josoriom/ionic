mod common;

use common::assertions::*;
use common::{parse_fixture, spectra};

#[test]
#[ignore = "manual semantic audit on medium fixture; parser still loads whole file into RAM"]
fn medium_fixture_full_pipeline() {
    let mzml =
        parse_fixture("inputs/covid19_biogune_MS_AA_PAI04_COVp20_220121_COV02001_19S20575_21.mzML");
    assert_semantic_roundtrip_full_pipeline(&mzml, 12, "medium-covid19-full-pipeline");
}

#[test]
#[ignore = "manual memory-heavy smoke; parser currently loads the full file into RAM"]
fn medium_fixture_parse_smoke() {
    let mzml =
        parse_fixture("inputs/covid19_biogune_MS_AA_PAI04_COVp20_220121_COV02001_19S20575_21.mzML");
    assert_declared_counts_consistent(&mzml);
    assert!(
        !spectra(&mzml).is_empty(),
        "medium fixture should contain spectra"
    );
    assert_all_refs_resolved(&mzml);
}
