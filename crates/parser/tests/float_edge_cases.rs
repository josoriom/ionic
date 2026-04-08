//! T2-2: Float edge cases via proptest.
//!
//! Exercises the Ion encoder→decoder roundtrip with NaN, ±Inf, subnormal,
//! -0.0, and arbitrary floats to ensure no data is silently corrupted.

mod common;

use common::binary_ext::BinaryDataExt;
use common::builders::{minimal_file_description, synthetic_binary_data_array};
use ionic::ion::{Decoder, WritingMode, encode};
use ionic::mzml::structs::*;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal MzML with a single spectrum carrying one f64 array.
fn mzml_with_f64_array(values: Vec<f64>) -> MzML {
    let len = values.len();
    MzML {
        file_description: Some(minimal_file_description()),
        run: Run {
            id: "float-test".to_string(),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: Some(len),
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(1),
                        binary_data_arrays: vec![synthetic_binary_data_array(
                            "MS:1000514", // m/z array
                            NumericType::Float64,
                            BinaryData::F64(values),
                            Some(len),
                        )],
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Build a minimal MzML with a single spectrum carrying one f32 array.
fn mzml_with_f32_array(values: Vec<f32>) -> MzML {
    let len = values.len();
    MzML {
        file_description: Some(minimal_file_description()),
        run: Run {
            id: "float-test-f32".to_string(),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: Some(len),
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(1),
                        binary_data_arrays: vec![synthetic_binary_data_array(
                            "MS:1000514",
                            NumericType::Float32,
                            BinaryData::F32(values),
                            Some(len),
                        )],
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Encode to Ion bytes and decode back to MzML.
fn roundtrip_ion(mzml: &MzML) -> MzML {
    let mut buf = Vec::new();
    encode(mzml, 0, false, WritingMode::Memory, &mut buf).expect("encode should succeed");
    let mut decoder = Decoder::open(&buf).expect("decoder open should succeed");
    decoder.to_mzml().expect("to_mzml should succeed")
}

/// Extract the first binary array's raw data from the first spectrum.
fn first_array_binary(mzml: &MzML) -> &BinaryData {
    mzml.run.spectrum_list.as_ref().unwrap().spectra[0]
        .binary_data_array_list
        .as_ref()
        .unwrap()
        .binary_data_arrays[0]
        .binary
        .as_ref()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Deterministic edge-case tests
// ---------------------------------------------------------------------------

#[test]
fn f64_nan_roundtrip() {
    let input = vec![f64::NAN, 1.0, f64::NAN];
    let mzml = mzml_with_f64_array(input);
    let out = roundtrip_ion(&mzml);
    let bin = first_array_binary(&out);
    let vals = bin.to_f64_vec();
    assert_eq!(vals.len(), 3);
    assert!(vals[0].is_nan(), "first element should be NaN");
    assert_eq!(vals[1], 1.0);
    assert!(vals[2].is_nan(), "third element should be NaN");
}

#[test]
fn f64_inf_roundtrip() {
    let input = vec![f64::INFINITY, f64::NEG_INFINITY, 0.0];
    let mzml = mzml_with_f64_array(input.clone());
    let out = roundtrip_ion(&mzml);
    let bin = first_array_binary(&out);
    let vals = bin.to_f64_vec();
    assert_eq!(vals.len(), 3);
    assert_eq!(vals[0], f64::INFINITY);
    assert_eq!(vals[1], f64::NEG_INFINITY);
    assert_eq!(vals[2], 0.0);
}

#[test]
fn f64_negative_zero_roundtrip() {
    let input = vec![-0.0_f64, 0.0_f64];
    let mzml = mzml_with_f64_array(input);
    let out = roundtrip_ion(&mzml);
    let bin = first_array_binary(&out);
    let vals = bin.to_f64_vec();
    assert_eq!(vals.len(), 2);
    // -0.0 and 0.0 compare equal, but their bits differ
    assert!(vals[0].is_sign_negative(), "-0.0 sign should be preserved");
    assert!(vals[1].is_sign_positive(), "0.0 sign should be preserved");
}

#[test]
fn f64_subnormal_roundtrip() {
    let input = vec![f64::MIN_POSITIVE / 2.0, -f64::MIN_POSITIVE / 2.0];
    assert!(input[0].is_subnormal(), "test value should be subnormal");
    let mzml = mzml_with_f64_array(input.clone());
    let out = roundtrip_ion(&mzml);
    let bin = first_array_binary(&out);
    let vals = bin.to_f64_vec();
    assert_eq!(vals.len(), 2);
    assert_eq!(vals[0].to_bits(), input[0].to_bits());
    assert_eq!(vals[1].to_bits(), input[1].to_bits());
}

#[test]
fn f64_max_min_roundtrip() {
    let input = vec![f64::MAX, f64::MIN, f64::EPSILON];
    let mzml = mzml_with_f64_array(input.clone());
    let out = roundtrip_ion(&mzml);
    let bin = first_array_binary(&out);
    let vals = bin.to_f64_vec();
    assert_eq!(vals, input);
}

#[test]
fn f32_special_values_roundtrip() {
    let input = vec![
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -0.0_f32,
        f32::MIN_POSITIVE / 2.0,
        f32::MAX,
        f32::MIN,
        f32::EPSILON,
    ];
    let mzml = mzml_with_f32_array(input.clone());
    let out = roundtrip_ion(&mzml);
    let bin = first_array_binary(&out);
    // After roundtrip, f32 arrays may come back as f32 or f64 depending on encoder.
    // Compare via bit patterns when possible.
    match bin {
        BinaryData::F32(vals) => {
            assert_eq!(vals.len(), input.len());
            for (i, (got, expected)) in vals.iter().zip(input.iter()).enumerate() {
                if expected.is_nan() {
                    assert!(got.is_nan(), "index {i}: expected NaN");
                } else {
                    assert_eq!(got.to_bits(), expected.to_bits(), "index {i}: bit mismatch");
                }
            }
        }
        BinaryData::F64(vals) => {
            // If encoder promoted to f64, compare with promoted values
            assert_eq!(vals.len(), input.len());
            for (i, (got, expected)) in vals.iter().zip(input.iter()).enumerate() {
                if expected.is_nan() {
                    assert!(got.is_nan(), "index {i}: expected NaN");
                } else {
                    assert_eq!(*got, *expected as f64, "index {i}: value mismatch");
                }
            }
        }
        other => panic!("unexpected binary variant: {}", other.variant_name()),
    }
}

#[test]
fn empty_f64_array_roundtrip() {
    let input: Vec<f64> = vec![];
    let mzml = mzml_with_f64_array(input);
    let out = roundtrip_ion(&mzml);
    let empty = vec![];
    let spectra = out
        .run
        .spectrum_list
        .as_ref()
        .map(|sl| &sl.spectra)
        .unwrap_or(&empty);
    // An empty array either roundtrips as empty or the spectrum has no arrays.
    if !spectra.is_empty()
        && let Some(bdal) = &spectra[0].binary_data_array_list
    {
        for bda in &bdal.binary_data_arrays {
            if let Some(bin) = &bda.binary {
                assert_eq!(bin.len(), 0, "empty array should roundtrip as empty");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property-based tests (proptest)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn proptest_f64_array_roundtrip(
        values in prop::collection::vec(prop::num::f64::ANY, 0..128)
    ) {
        let mzml = mzml_with_f64_array(values.clone());
        let out = roundtrip_ion(&mzml);
        if values.is_empty() {
            // Empty arrays may be omitted entirely — that's acceptable
            return Ok(());
        }
        let bin = first_array_binary(&out);
        let result = bin.to_f64_vec();
        prop_assert_eq!(result.len(), values.len());
        for (i, (got, expected)) in result.iter().zip(values.iter()).enumerate() {
            if expected.is_nan() {
                prop_assert!(got.is_nan(), "index {}: expected NaN, got {}", i, got);
            } else {
                prop_assert_eq!(
                    got.to_bits(), expected.to_bits(),
                    "index {}: bit mismatch ({} vs {})", i, got, expected
                );
            }
        }
    }

    #[test]
    fn proptest_f32_array_roundtrip(
        values in prop::collection::vec(prop::num::f32::ANY, 0..128)
    ) {
        let mzml = mzml_with_f32_array(values.clone());
        let out = roundtrip_ion(&mzml);
        if values.is_empty() {
            return Ok(());
        }
        let bin = first_array_binary(&out);
        let result_f64 = bin.to_f64_vec();
        prop_assert_eq!(result_f64.len(), values.len());
        for (i, (got, expected)) in result_f64.iter().zip(values.iter()).enumerate() {
            let expected_f64 = *expected as f64;
            if expected.is_nan() {
                prop_assert!(got.is_nan(), "index {}: expected NaN, got {}", i, got);
            } else {
                prop_assert_eq!(
                    *got, expected_f64,
                    "index {}: value mismatch ({} vs {})", i, got, expected_f64
                );
            }
        }
    }
}
