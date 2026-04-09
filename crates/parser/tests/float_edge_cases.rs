//! Exercises the Ion encoder→decoder roundtrip

mod common;

use common::binary_ext::BinaryDataExt;
use common::helpers::mzml_with_single_array;
use common::{first_spectrum_binary, roundtrip};
use ionic::mzml::structs::*;
use proptest::prelude::*;

#[test]
fn f64_nan_roundtrip() {
    let input = vec![f64::NAN, 1.0, f64::NAN];
    let len = input.len();
    let mzml = mzml_with_single_array(NumericType::Float64, BinaryData::F64(input.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("should have binary data");
    let vals = bin.to_f64_vec();
    assert_eq!(vals.len(), 3);
    assert!(vals[0].is_nan(), "first element should be NaN");
    assert_eq!(vals[1], 1.0);
    assert!(vals[2].is_nan(), "third element should be NaN");
}

#[test]
fn f64_inf_roundtrip() {
    let input = vec![f64::INFINITY, f64::NEG_INFINITY, 0.0];
    let len = input.len();
    let mzml = mzml_with_single_array(NumericType::Float64, BinaryData::F64(input.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("should have binary data");
    let vals = bin.to_f64_vec();
    assert_eq!(vals.len(), 3);
    assert_eq!(vals[0], f64::INFINITY);
    assert_eq!(vals[1], f64::NEG_INFINITY);
    assert_eq!(vals[2], 0.0);
}

#[test]
fn f64_negative_zero_roundtrip() {
    let input = vec![-0.0_f64, 0.0_f64];
    let len = input.len();
    let mzml = mzml_with_single_array(NumericType::Float64, BinaryData::F64(input.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("should have binary data");
    let vals = bin.to_f64_vec();
    assert_eq!(vals.len(), 2);
    assert!(vals[0].is_sign_negative(), "-0.0 sign should be preserved");
    assert!(vals[1].is_sign_positive(), "0.0 sign should be preserved");
}

#[test]
fn f64_subnormal_roundtrip() {
    let input = vec![f64::MIN_POSITIVE / 2.0, -f64::MIN_POSITIVE / 2.0];
    assert!(input[0].is_subnormal(), "test value should be subnormal");
    let len = input.len();
    let mzml = mzml_with_single_array(NumericType::Float64, BinaryData::F64(input.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("should have binary data");
    let vals = bin.to_f64_vec();
    assert_eq!(vals.len(), 2);
    assert_eq!(vals[0].to_bits(), input[0].to_bits());
    assert_eq!(vals[1].to_bits(), input[1].to_bits());
}

#[test]
fn f64_max_min_roundtrip() {
    let input = vec![f64::MAX, f64::MIN, f64::EPSILON];
    let len = input.len();
    let mzml = mzml_with_single_array(NumericType::Float64, BinaryData::F64(input.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("should have binary data");
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
    let len = input.len();
    let mzml = mzml_with_single_array(NumericType::Float32, BinaryData::F32(input.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("should have binary data");
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
    let len = input.len();
    let mzml = mzml_with_single_array(NumericType::Float64, BinaryData::F64(input.clone()), len);
    let out = roundtrip(&mzml);
    let empty = vec![];
    let spectra = out
        .run
        .spectrum_list
        .as_ref()
        .map(|sl| &sl.spectra)
        .unwrap_or(&empty);
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn proptest_f64_array_roundtrip(
        values in prop::collection::vec(prop::num::f64::ANY, 0..128)
    ) {
        let len = values.len();
        let mzml = mzml_with_single_array(NumericType::Float64, BinaryData::F64(values.clone()), len);
        let out = roundtrip(&mzml);
        if values.is_empty() {
            return Ok(());
        }
        let bin = first_spectrum_binary(&out).expect("should have binary data");
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
        let len = values.len();
        let mzml = mzml_with_single_array(NumericType::Float32, BinaryData::F32(values.clone()), len);
        let out = roundtrip(&mzml);
        if values.is_empty() {
            return Ok(());
        }
        let bin = first_spectrum_binary(&out).expect("should have binary data");
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
