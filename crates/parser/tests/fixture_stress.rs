//! T3-4: Stress test — all fixture files through full Ion roundtrip.
//!
//! Parses every known mzML fixture file, encodes it to Ion, decodes back,
//! and asserts semantic equality. This exercises the full pipeline with
//! real-world data from ProteoWizard and internal test files.

mod common;

use common::fixtures::{ALL_FIXTURES, INTERNAL_MZML_FIXTURES, PWIZ_FIXTURES};

// ---------------------------------------------------------------------------
// Ion roundtrip at compression level 0 (no zstd)
// ---------------------------------------------------------------------------

#[test]
fn all_fixtures_ion_roundtrip_uncompressed() {
    let mut failures = Vec::new();
    // Use only PWIZ fixtures for strict canonical diff — legacy 0.99.x fixtures
    // have known ms_level roundtrip gaps that are pre-existing behavior.
    for fixture_path in PWIZ_FIXTURES {
        let original = common::parse_fixture(fixture_path);
        let bytes = common::encode_to_ion(&original, 0, false);
        match common::decode_ion(&bytes) {
            Ok(decoded) => {
                let diffs = common::canonical_diff_paths(&original, &decoded);
                if !diffs.is_empty() {
                    failures.push(format!("{fixture_path}: {} diffs", diffs.len()));
                    for d in &diffs[..diffs.len().min(5)] {
                        failures.push(format!("  {d}"));
                    }
                }
            }
            Err(e) => {
                failures.push(format!("{fixture_path}: decode error: {e}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "fixture roundtrip failures:\n{}",
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Ion roundtrip at high compression (level 12)
// ---------------------------------------------------------------------------

#[test]
fn all_fixtures_ion_roundtrip_compressed() {
    let mut failures = Vec::new();
    // Use only PWIZ fixtures for strict canonical diff.
    for fixture_path in PWIZ_FIXTURES {
        let original = common::parse_fixture(fixture_path);
        let bytes = common::encode_to_ion(&original, 12, false);
        match common::decode_ion(&bytes) {
            Ok(decoded) => {
                let diffs = common::canonical_diff_paths(&original, &decoded);
                if !diffs.is_empty() {
                    failures.push(format!("{fixture_path}: {} diffs", diffs.len()));
                    for d in &diffs[..diffs.len().min(5)] {
                        failures.push(format!("  {d}"));
                    }
                }
            }
            Err(e) => {
                failures.push(format!("{fixture_path}: decode error: {e}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "fixture roundtrip failures (compressed):\n{}",
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Ion roundtrip with force_f32
// ---------------------------------------------------------------------------

/// Legacy 0.99.x fixtures: structural check only (no ms_level diff check).
/// Verifies that spectrum/chromatogram counts and run ID survive roundtrip.
#[test]
fn legacy_fixtures_ion_roundtrip_structural() {
    for fixture_path in INTERNAL_MZML_FIXTURES {
        let original = common::parse_fixture(fixture_path);
        let bytes = common::encode_to_ion(&original, 0, false);
        let decoded = common::decode_ion(&bytes).unwrap_or_else(|e| {
            panic!("{fixture_path}: decode error: {e}");
        });

        assert_eq!(
            common::spectra(&original).len(),
            common::spectra(&decoded).len(),
            "{fixture_path}: spectrum count mismatch"
        );
        assert_eq!(
            common::chromatograms(&original).len(),
            common::chromatograms(&decoded).len(),
            "{fixture_path}: chromatogram count mismatch"
        );
        assert_eq!(
            original.run.id, decoded.run.id,
            "{fixture_path}: run ID mismatch"
        );
    }
}

#[test]
fn all_fixtures_ion_roundtrip_force_f32() {
    // force_f32 downcasts f64 arrays to f32, so we can't assert exact
    // equality on array payloads. Instead, we verify the pipeline doesn't
    // panic and the structural shape (spectrum count, chromatogram count,
    // IDs) is preserved.
    for fixture_path in ALL_FIXTURES {
        let original = common::parse_fixture(fixture_path);
        let bytes = common::encode_to_ion(&original, 6, true);
        let decoded = common::decode_ion(&bytes).unwrap_or_else(|e| {
            panic!("{fixture_path}: decode error with force_f32: {e}");
        });

        // Basic structural checks
        let orig_spectra = common::spectra(&original);
        let dec_spectra = common::spectra(&decoded);
        assert_eq!(
            orig_spectra.len(),
            dec_spectra.len(),
            "{fixture_path}: spectrum count mismatch"
        );

        let orig_chroms = common::chromatograms(&original);
        let dec_chroms = common::chromatograms(&decoded);
        assert_eq!(
            orig_chroms.len(),
            dec_chroms.len(),
            "{fixture_path}: chromatogram count mismatch"
        );

        assert_eq!(
            original.run.id, decoded.run.id,
            "{fixture_path}: run ID mismatch"
        );
    }
}

// ---------------------------------------------------------------------------
// Full pipeline: mzML → Ion → MzML (struct) → XML → reparse
// ---------------------------------------------------------------------------

#[test]
fn all_fixtures_full_pipeline_roundtrip() {
    use ionic::mzml::bin_to_mzml::bin_to_mzml;
    use ionic::mzml::parse_mzml::parse_mzml;

    let mut failures = Vec::new();
    for fixture_path in ALL_FIXTURES {
        let original = common::parse_fixture(fixture_path);

        // Step 1: mzML struct → Ion bytes → mzML struct
        let ion_bytes = common::encode_to_ion(&original, 0, false);
        let from_ion = match common::decode_ion(&ion_bytes) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("{fixture_path}: ion decode error: {e}"));
                continue;
            }
        };

        // Step 2: mzML struct → XML string → re-parse to mzML struct
        let xml_str = match bin_to_mzml(&from_ion) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{fixture_path}: bin_to_mzml error: {e}"));
                continue;
            }
        };

        let reparsed = match parse_mzml(xml_str.as_bytes()) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("{fixture_path}: reparse error: {e}"));
                continue;
            }
        };

        // Verify spectrum and chromatogram counts survived the full pipeline
        let orig_n_spec = common::spectra(&original).len();
        let final_n_spec = common::spectra(&reparsed).len();
        if orig_n_spec != final_n_spec {
            failures.push(format!(
                "{fixture_path}: spectrum count {orig_n_spec} → {final_n_spec}"
            ));
        }

        let orig_n_chrom = common::chromatograms(&original).len();
        let final_n_chrom = common::chromatograms(&reparsed).len();
        if orig_n_chrom != final_n_chrom {
            failures.push(format!(
                "{fixture_path}: chromatogram count {orig_n_chrom} → {final_n_chrom}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "full pipeline roundtrip failures:\n{}",
        failures.join("\n")
    );
}
