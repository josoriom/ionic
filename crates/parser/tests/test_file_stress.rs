//! Parses available known mzML tests file, encodes it to Ion, decodes back,
//! and asserts semantic equality.

mod common;

use common::test_files::{ALL_TEST_FILES, INTERNAL_MZML_TEST_FILES, PWIZ_TEST_FILES};

#[test]
fn all_test_files_ion_roundtrip_uncompressed() {
    let mut failures = Vec::new();
    for test_file_path in PWIZ_TEST_FILES {
        let original = common::parse_test_file(test_file_path);
        let bytes = common::encode_to_ion(&original, 0, false);
        match common::decode_ion(&bytes) {
            Ok(decoded) => {
                let diffs = common::canonical_diff_paths(&original, &decoded);
                if !diffs.is_empty() {
                    failures.push(format!("{test_file_path}: {} diffs", diffs.len()));
                    for d in &diffs[..diffs.len().min(5)] {
                        failures.push(format!("  {d}"));
                    }
                }
            }
            Err(e) => {
                failures.push(format!("{test_file_path}: decode error: {e}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "test_file roundtrip failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn all_test_files_ion_roundtrip_compressed() {
    let mut failures = Vec::new();
    for test_file_path in PWIZ_TEST_FILES {
        let original = common::parse_test_file(test_file_path);
        let bytes = common::encode_to_ion(&original, 12, false);
        match common::decode_ion(&bytes) {
            Ok(decoded) => {
                let diffs = common::canonical_diff_paths(&original, &decoded);
                if !diffs.is_empty() {
                    failures.push(format!("{test_file_path}: {} diffs", diffs.len()));
                    for d in &diffs[..diffs.len().min(5)] {
                        failures.push(format!("  {d}"));
                    }
                }
            }
            Err(e) => {
                failures.push(format!("{test_file_path}: decode error: {e}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "test_file roundtrip failures (compressed):\n{}",
        failures.join("\n")
    );
}

#[test]
fn legacy_test_files_ion_roundtrip_structural() {
    for test_file_path in INTERNAL_MZML_TEST_FILES {
        let original = common::parse_test_file(test_file_path);
        let bytes = common::encode_to_ion(&original, 0, false);
        let decoded = common::decode_ion(&bytes).unwrap_or_else(|e| {
            panic!("{test_file_path}: decode error: {e}");
        });

        assert_eq!(
            common::spectra(&original).len(),
            common::spectra(&decoded).len(),
            "{test_file_path}: spectrum count mismatch"
        );
        assert_eq!(
            common::chromatograms(&original).len(),
            common::chromatograms(&decoded).len(),
            "{test_file_path}: chromatogram count mismatch"
        );
        assert_eq!(
            original.run.id, decoded.run.id,
            "{test_file_path}: run ID mismatch"
        );
    }
}

#[test]
fn all_test_files_ion_roundtrip_force_f32() {
    for test_file_path in ALL_TEST_FILES {
        let original = common::parse_test_file(test_file_path);
        let bytes = common::encode_to_ion(&original, 6, true);
        let decoded = common::decode_ion(&bytes).unwrap_or_else(|e| {
            panic!("{test_file_path}: decode error with force_f32: {e}");
        });

        let orig_spectra = common::spectra(&original);
        let dec_spectra = common::spectra(&decoded);
        assert_eq!(
            orig_spectra.len(),
            dec_spectra.len(),
            "{test_file_path}: spectrum count mismatch"
        );

        let orig_chroms = common::chromatograms(&original);
        let dec_chroms = common::chromatograms(&decoded);
        assert_eq!(
            orig_chroms.len(),
            dec_chroms.len(),
            "{test_file_path}: chromatogram count mismatch"
        );

        assert_eq!(
            original.run.id, decoded.run.id,
            "{test_file_path}: run ID mismatch"
        );
    }
}

// Full roundtrip: mzML → Ion → MzML (struct) → XML → reparse
#[test]
fn all_test_files_full_roundtrip() {
    use ionic::mzml::bin_to_mzml::bin_to_mzml;
    use ionic::mzml::parse_mzml::parse_mzml;

    let mut failures = Vec::new();
    for test_file_path in ALL_TEST_FILES {
        let original = common::parse_test_file(test_file_path);

        // Step 1: mzML struct → Ion bytes → mzML struct
        let ion_bytes = common::encode_to_ion(&original, 0, false);
        let from_ion = match common::decode_ion(&ion_bytes) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("{test_file_path}: ion decode error: {e}"));
                continue;
            }
        };

        // Step 2: mzML struct → XML string → re-parse to mzML struct
        let xml_str = match bin_to_mzml(&from_ion) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{test_file_path}: bin_to_mzml error: {e}"));
                continue;
            }
        };

        let reparsed = match parse_mzml(xml_str.as_bytes()) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("{test_file_path}: reparse error: {e}"));
                continue;
            }
        };

        let orig_n_spec = common::spectra(&original).len();
        let final_n_spec = common::spectra(&reparsed).len();
        if orig_n_spec != final_n_spec {
            failures.push(format!(
                "{test_file_path}: spectrum count {orig_n_spec} → {final_n_spec}"
            ));
        }

        let orig_n_chrom = common::chromatograms(&original).len();
        let final_n_chrom = common::chromatograms(&reparsed).len();
        if orig_n_chrom != final_n_chrom {
            failures.push(format!(
                "{test_file_path}: chromatogram count {orig_n_chrom} → {final_n_chrom}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "full pipeline roundtrip failures:\n{}",
        failures.join("\n")
    );
}
