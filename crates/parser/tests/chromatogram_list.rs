mod common;

use common::assertions::*;
use common::fixtures;
use common::{chromatogram_arrays, chromatogram_by_id, first_chrom_array_values_by_accession};

#[test]
fn tiny_11_identity_and_shapes() {
    let mzml = fixtures::tiny_pwiz_11();
    let cl = mzml
        .run
        .chromatogram_list
        .as_ref()
        .expect("chromatogramList parsed");
    assert_eq!(cl.chromatograms.len(), 2);
    let tic = chromatogram_by_id(mzml, "tic");
    assert_eq!(tic.default_array_length, Some(15));
    assert_eq!(chromatogram_arrays(tic).len(), 2);
    let sic = chromatogram_by_id(mzml, "sic");
    assert_eq!(sic.default_array_length, Some(10));
    assert_eq!(chromatogram_arrays(sic).len(), 2);
}

#[test]
fn tiny_10_identity_and_native_ids() {
    let mzml = fixtures::tiny_pwiz_10();
    let tic = chromatogram_by_id(mzml, "tic");
    assert_eq!(tic.native_id.as_deref(), Some("tic native"));
    assert_eq!(tic.default_array_length, Some(15));
    let sic = chromatogram_by_id(mzml, "sic");
    assert_eq!(sic.native_id.as_deref(), Some("sic native"));
    assert_eq!(sic.default_array_length, Some(10));
}

#[test]
fn small_11_identity() {
    let mzml = fixtures::small_pwiz_11();
    let cl = mzml
        .run
        .chromatogram_list
        .as_ref()
        .expect("chromatogramList parsed");
    assert_eq!(cl.chromatograms.len(), 1);
    assert_eq!(cl.chromatograms[0].id, "TIC");
    assert_eq!(cl.chromatograms[0].default_array_length, Some(48));
}

#[test]
fn tiny_11_tic_pairwise_values() {
    let mzml = fixtures::tiny_pwiz_11();
    let tic = chromatogram_by_id(mzml, "tic");
    let t = first_chrom_array_values_by_accession(tic, "MS:1000595");
    let i = first_chrom_array_values_by_accession(tic, "MS:1000515");
    assert_eq!(t.len(), 15);
    assert_eq!(i.len(), 15);
    for idx in 0..15 {
        rel_close_f64(t[idx], idx as f64, EPS_REL_F64, &format!("tic time[{idx}]"));
        rel_close_f64(
            i[idx],
            (15 - idx) as f64,
            EPS_REL_F64,
            &format!("tic intensity[{idx}]"),
        );
    }
}

#[test]
fn tiny_11_sic_pairwise_values() {
    let mzml = fixtures::tiny_pwiz_11();
    let sic = chromatogram_by_id(mzml, "sic");
    let t = first_chrom_array_values_by_accession(sic, "MS:1000595");
    let i = first_chrom_array_values_by_accession(sic, "MS:1000515");
    assert_eq!(t.len(), 10);
    assert_eq!(i.len(), 10);
    for idx in 0..10 {
        rel_close_f64(t[idx], idx as f64, EPS_REL_F64, &format!("sic time[{idx}]"));
        rel_close_f64(
            i[idx],
            (10 - idx) as f64,
            EPS_REL_F64,
            &format!("sic intensity[{idx}]"),
        );
    }
}
