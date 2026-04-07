mod common;

use common::assertions::*;
use common::fixtures;
use common::{
    find_array_by_accession, find_name_value_indices, find_spot_id_indices,
    first_array_values_by_accession, spectrum_arrays, spectrum_by_id, BinaryDataExt,
};

#[test]
fn tiny_11_identity_and_counts() {
    let mzml = fixtures::tiny_pwiz_11();
    let sl = mzml
        .run
        .spectrum_list
        .as_ref()
        .expect("spectrumList parsed");
    assert_eq!(sl.spectra.len(), 4);
    let ids: Vec<_> = sl.spectra.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids[0], "scan=19");
    assert_eq!(ids[1], "scan=20");
    assert_eq!(ids[2], "scan=21");
    assert_eq!(ids[3], "sample=1 period=1 cycle=22 experiment=1");
    assert_eq!(sl.spectra[3].spot_id.as_deref(), Some("A1,42x42,4242x4242"));
}

#[test]
fn tiny_10_identity_and_counts() {
    let mzml = fixtures::tiny_pwiz_10();
    let sl = mzml
        .run
        .spectrum_list
        .as_ref()
        .expect("spectrumList parsed");
    assert_eq!(sl.spectra.len(), 4);
    let ids: Vec<_> = sl.spectra.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["S19", "S20", "S21", "S22"]);
    assert_eq!(sl.spectra[3].spot_id.as_deref(), Some("A1,42x42,4242x4242"));
}

#[test]
fn small_11_first_last_identity() {
    let mzml = fixtures::small_pwiz_11();
    let sl = mzml
        .run
        .spectrum_list
        .as_ref()
        .expect("spectrumList parsed");
    assert_eq!(sl.spectra.len(), 48);
    assert_eq!(sl.spectra.first().map(|s| s.index), Some(Some(0)));
    assert_eq!(
        sl.spectra.first().map(|s| s.id.as_str()),
        Some("controllerType=0 controllerNumber=1 scan=1")
    );
    assert_eq!(sl.spectra.last().map(|s| s.index), Some(Some(47)));
    assert_eq!(
        sl.spectra.last().map(|s| s.id.as_str()),
        Some("controllerType=0 controllerNumber=1 scan=48")
    );
}

#[test]
fn tiny_11_binary_payload_shapes() {
    let mzml = fixtures::tiny_pwiz_11();
    let s19 = spectrum_by_id(mzml, "scan=19");
    assert_eq!(s19.default_array_length, Some(15));
    let a19 = spectrum_arrays(s19);
    assert_eq!(a19.len(), 2);
    assert_eq!(
        find_array_by_accession(a19, "MS:1000514")
            .binary
            .as_ref()
            .expect("mz binary")
            .first_f64(),
        Some(0.0)
    );
    assert_eq!(
        find_array_by_accession(a19, "MS:1000515")
            .binary
            .as_ref()
            .expect("intensity binary")
            .first_f64(),
        Some(15.0)
    );

    let s20 = spectrum_by_id(mzml, "scan=20");
    assert_eq!(s20.default_array_length, Some(10));
    let a20 = spectrum_arrays(s20);
    assert_eq!(a20.len(), 2);
    assert_eq!(
        find_array_by_accession(a20, "MS:1000514")
            .binary
            .as_ref()
            .expect("mz binary")
            .first_f64(),
        Some(0.0)
    );
    assert_eq!(
        find_array_by_accession(a20, "MS:1000515")
            .binary
            .as_ref()
            .expect("intensity binary")
            .first_f64(),
        Some(20.0)
    );
}

#[test]
fn tiny_11_precursor_ref_integrity() {
    let mzml = fixtures::tiny_pwiz_11();
    let s20 = spectrum_by_id(mzml, "scan=20");
    let precursor_list = s20
        .precursor_list
        .as_ref()
        .expect("scan=20 precursorList parsed");
    assert_eq!(precursor_list.precursors.len(), 1);
    assert_eq!(
        precursor_list.precursors[0].spectrum_ref.as_deref(),
        Some("scan=19")
    );
}

#[test]
fn tiny_11_find_name_value_equivalence() {
    let mzml = fixtures::tiny_pwiz_11();
    assert_eq!(find_name_value_indices(mzml, "scan", "19"), vec![0]);
    assert_eq!(find_name_value_indices(mzml, "scan", "20"), vec![1]);
    assert_eq!(find_name_value_indices(mzml, "scan", "21"), vec![2]);
    assert_eq!(find_name_value_indices(mzml, "sample", "1"), vec![3]);
    assert_eq!(find_name_value_indices(mzml, "period", "1"), vec![3]);
    assert_eq!(find_name_value_indices(mzml, "cycle", "22"), vec![3]);
    assert_eq!(find_name_value_indices(mzml, "experiment", "1"), vec![3]);
}

#[test]
fn tiny_11_spot_id_lookup_equivalence() {
    let mzml = fixtures::tiny_pwiz_11();
    assert!(
        find_spot_id_indices(mzml, "A1").is_empty(),
        "partial spot id should not match"
    );
    assert_eq!(find_spot_id_indices(mzml, "A1,42x42,4242x4242"), vec![3]);
}

#[test]
fn tiny_11_scan_param_group_ref_equivalence() {
    let mzml = fixtures::tiny_pwiz_11();
    let s19 = spectrum_by_id(mzml, "scan=19");
    let s20 = spectrum_by_id(mzml, "scan=20");
    assert_eq!(s19.referenceable_param_group_refs.len(), 1);
    assert_eq!(
        s19.referenceable_param_group_refs[0].r#ref,
        "CommonMS1SpectrumParams"
    );
    assert_eq!(s20.referenceable_param_group_refs.len(), 1);
    assert_eq!(
        s20.referenceable_param_group_refs[0].r#ref,
        "CommonMS2SpectrumParams"
    );
}

#[test]
fn tiny_11_s19_pairwise_binary_values() {
    let mzml = fixtures::tiny_pwiz_11();
    let s19 = spectrum_by_id(mzml, "scan=19");
    let mz = first_array_values_by_accession(s19, "MS:1000514");
    let intensity = first_array_values_by_accession(s19, "MS:1000515");
    assert_eq!(mz.len(), 15);
    assert_eq!(intensity.len(), 15);
    for i in 0..15 {
        rel_close_f64(mz[i], i as f64, EPS_REL_F64, &format!("scan=19 mz[{i}]"));
        rel_close_f64(
            intensity[i],
            (15 - i) as f64,
            EPS_REL_F64,
            &format!("scan=19 intensity[{i}]"),
        );
    }
}

#[test]
fn tiny_11_s20_pairwise_binary_values() {
    let mzml = fixtures::tiny_pwiz_11();
    let s20 = spectrum_by_id(mzml, "scan=20");
    let mz = first_array_values_by_accession(s20, "MS:1000514");
    let intensity = first_array_values_by_accession(s20, "MS:1000515");
    assert_eq!(mz.len(), 10);
    assert_eq!(intensity.len(), 10);
    for i in 0..10 {
        rel_close_f64(
            mz[i],
            (2 * i) as f64,
            EPS_REL_F64,
            &format!("scan=20 mz[{i}]"),
        );
        rel_close_f64(
            intensity[i],
            (2 * (10 - i)) as f64,
            EPS_REL_F64,
            &format!("scan=20 intensity[{i}]"),
        );
    }
}
