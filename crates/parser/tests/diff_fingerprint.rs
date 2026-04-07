mod common;

use common::fixtures;
use common::{
    canonical_diff_paths, chromatogram_arrays, chromatograms, semantic_fingerprint, spectra,
    spectrum_arrays, BinaryDataExt,
};

#[test]
fn fingerprint_is_stable_for_identical_input() {
    let a = fixtures::tiny_pwiz_11().clone();
    let b = fixtures::tiny_pwiz_11().clone();
    assert_eq!(semantic_fingerprint(&a), semantic_fingerprint(&b));
}

#[test]
fn fingerprint_changes_on_critical_mutation() {
    let a = fixtures::tiny_pwiz_11().clone();
    let mut b = fixtures::tiny_pwiz_11().clone();
    b.run.id.push_str("_mut");
    assert_ne!(semantic_fingerprint(&a), semantic_fingerprint(&b));
    let diffs = canonical_diff_paths(&a, &b);
    assert!(
        diffs.iter().any(|d| d.starts_with("run/id:")),
        "expected run/id in canonical diff, got: {diffs:#?}"
    );
}

#[test]
fn binary_only_equivalence_when_ignoring_identity() {
    let src = fixtures::tiny_pwiz_11();
    let mut modified = src.clone();
    modified.run.id = "modified-run".to_string();
    if let Some(sl) = modified.run.spectrum_list.as_mut() {
        for (i, s) in sl.spectra.iter_mut().enumerate() {
            s.id = format!("mut-spectrum-{i}");
            s.scan_number = None;
            s.ms_level = None;
        }
    }
    if let Some(cl) = modified.run.chromatogram_list.as_mut() {
        for (i, c) in cl.chromatograms.iter_mut().enumerate() {
            c.id = format!("mut-chromatogram-{i}");
        }
    }

    let src_spec_payloads: Vec<Vec<Vec<f64>>> = spectra(src)
        .iter()
        .map(|s| {
            spectrum_arrays(s)
                .iter()
                .filter_map(|a| a.binary.as_ref())
                .map(|b| b.to_f64_vec())
                .collect()
        })
        .collect();
    let modified_spec_payloads: Vec<Vec<Vec<f64>>> = spectra(&modified)
        .iter()
        .map(|s| {
            spectrum_arrays(s)
                .iter()
                .filter_map(|a| a.binary.as_ref())
                .map(|b| b.to_f64_vec())
                .collect()
        })
        .collect();

    let src_chrom_payloads: Vec<Vec<Vec<f64>>> = chromatograms(src)
        .iter()
        .map(|c| {
            chromatogram_arrays(c)
                .iter()
                .filter_map(|a| a.binary.as_ref())
                .map(|b| b.to_f64_vec())
                .collect()
        })
        .collect();
    let modified_chrom_payloads: Vec<Vec<Vec<f64>>> = chromatograms(&modified)
        .iter()
        .map(|c| {
            chromatogram_arrays(c)
                .iter()
                .filter_map(|a| a.binary.as_ref())
                .map(|b| b.to_f64_vec())
                .collect()
        })
        .collect();

    assert_eq!(src_spec_payloads, modified_spec_payloads);
    assert_eq!(src_chrom_payloads, modified_chrom_payloads);
}
