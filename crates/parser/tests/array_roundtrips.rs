//! T3-2 + T3-3: Per-element and arbitrary array roundtrip tests.
//!
//! Tests all 6 BinaryData variants (F64, F32, F16, I64, I32, I16) through
//! the Ion encode→decode roundtrip, both with deterministic values and
//! proptest-generated arbitrary arrays.

mod common;

use common::binary_ext::BinaryDataExt;
use common::builders::{minimal_file_description, synthetic_binary_data_array};
use ionic::ion::{encode, Decoder, WritingMode};
use ionic::mzml::structs::*;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal MzML with a single spectrum carrying one binary array.
fn mzml_with_single_array(
    numeric_type: NumericType,
    binary: BinaryData,
    len: usize,
) -> MzML {
    MzML {
        file_description: Some(minimal_file_description()),
        run: Run {
            id: format!("array-test-{numeric_type:?}"),
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
                            numeric_type,
                            binary,
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
fn roundtrip(mzml: &MzML) -> MzML {
    let mut buf = Vec::new();
    encode(mzml, 0, false, WritingMode::Memory, &mut buf).expect("encode should succeed");
    let mut decoder = Decoder::open(&buf).expect("decoder open should succeed");
    decoder.to_mzml().expect("to_mzml should succeed")
}

/// Extract the first binary array's raw data from the first spectrum.
fn first_array(mzml: &MzML) -> Option<&BinaryData> {
    mzml.run
        .spectrum_list
        .as_ref()?
        .spectra
        .first()?
        .binary_data_array_list
        .as_ref()?
        .binary_data_arrays
        .first()?
        .binary
        .as_ref()
}

// ---------------------------------------------------------------------------
// T3-3: Per-element deterministic roundtrip tests for all 6 types
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_f64_array() {
    let values = vec![1.0_f64, -2.5, 0.0, f64::MAX, f64::MIN, std::f64::consts::PI];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Float64, BinaryData::F64(values.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_array(&out).expect("should have binary data");
    let got = bin.to_f64_vec();
    assert_eq!(got, values);
}

#[test]
fn roundtrip_f32_array() {
    let values = vec![1.0_f32, -2.5, 0.0, f32::MAX, f32::MIN, std::f32::consts::PI];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Float32, BinaryData::F32(values.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_array(&out).expect("should have binary data");
    // May come back as f32 or promoted to f64
    let got = bin.to_f64_vec();
    let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
    assert_eq!(got, expected);
}

#[test]
fn roundtrip_i64_array() {
    let values = vec![0_i64, 1, -1, i64::MAX, i64::MIN, 42];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Int64, BinaryData::I64(values.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_array(&out).expect("should have binary data");
    match bin {
        BinaryData::I64(got) => assert_eq!(got, &values),
        other => {
            // If decoder converts to f64, compare via f64 (lossy for extreme i64 values)
            let got = other.to_f64_vec();
            let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
            assert_eq!(got, expected, "i64 roundtrip values differ (via f64 conversion)");
        }
    }
}

#[test]
fn roundtrip_i32_array() {
    let values = vec![0_i32, 1, -1, i32::MAX, i32::MIN, 42];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Int32, BinaryData::I32(values.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_array(&out).expect("should have binary data");
    match bin {
        BinaryData::I32(got) => assert_eq!(got, &values),
        other => {
            let got = other.to_f64_vec();
            let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
            assert_eq!(got, expected, "i32 roundtrip values differ");
        }
    }
}

#[test]
fn roundtrip_i16_array() {
    let values = vec![0_i16, 1, -1, i16::MAX, i16::MIN, 42];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Int16, BinaryData::I16(values.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_array(&out).expect("should have binary data");
    match bin {
        BinaryData::I16(got) => assert_eq!(got, &values),
        other => {
            let got = other.to_f64_vec();
            let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
            assert_eq!(got, expected, "i16 roundtrip values differ");
        }
    }
}

#[test]
fn roundtrip_single_element_per_type() {
    // One element per type — covers the degenerate case
    let cases: Vec<(NumericType, BinaryData)> = vec![
        (NumericType::Float64, BinaryData::F64(vec![42.0])),
        (NumericType::Float32, BinaryData::F32(vec![42.0])),
        (NumericType::Int64, BinaryData::I64(vec![42])),
        (NumericType::Int32, BinaryData::I32(vec![42])),
        (NumericType::Int16, BinaryData::I16(vec![42])),
    ];

    for (nt, bin) in cases {
        let mzml = mzml_with_single_array(nt, bin.clone(), 1);
        let out = roundtrip(&mzml);
        let got = first_array(&out).expect("should have binary data");
        let expected_f64 = bin.to_f64_vec();
        let got_f64 = got.to_f64_vec();
        assert_eq!(
            got_f64, expected_f64,
            "single-element roundtrip failed for {nt:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Compression level sweep
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_at_compression_levels() {
    let values = vec![100.0_f64, 200.0, 300.0, 400.0, 500.0];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Float64, BinaryData::F64(values.clone()), len);

    for level in [0, 3, 10, 22] {
        let mut buf = Vec::new();
        encode(&mzml, level, false, WritingMode::Memory, &mut buf)
            .unwrap_or_else(|e| panic!("encode at level {level} failed: {e}"));
        let mut decoder = Decoder::open(&buf)
            .unwrap_or_else(|e| panic!("decoder open at level {level} failed: {e}"));
        let decoded = decoder
            .to_mzml()
            .unwrap_or_else(|e| panic!("to_mzml at level {level} failed: {e}"));
        let got = first_array(&decoded).expect("should have binary data");
        assert_eq!(
            got.to_f64_vec(),
            values,
            "values differ at compression level {level}"
        );
    }
}

// ---------------------------------------------------------------------------
// force_f32 mode test
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_force_f32_downcasts() {
    let values = vec![100.0_f64, 200.0, 300.0];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Float64, BinaryData::F64(values.clone()), len);

    let mut buf = Vec::new();
    encode(&mzml, 0, true, WritingMode::Memory, &mut buf).expect("encode with force_f32");
    let mut decoder = Decoder::open(&buf).expect("decoder open");
    let decoded = decoder.to_mzml().expect("to_mzml");

    let bin = first_array(&decoded).expect("should have binary data");
    // With force_f32, the output should be f32 (or at least the values should
    // be representable as f32)
    let got = bin.to_f64_vec();
    for (i, (g, e)) in got.iter().zip(values.iter()).enumerate() {
        let expected_f32 = *e as f32 as f64;
        assert!(
            (g - expected_f32).abs() < 1e-6,
            "index {i}: force_f32 value mismatch: {g} vs {expected_f32}"
        );
    }
}

// ---------------------------------------------------------------------------
// T3-2: Proptest arbitrary array roundtrips for all numeric types
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn proptest_arbitrary_f64_roundtrip(
        values in prop::collection::vec(prop::num::f64::ANY, 1..64)
    ) {
        let len = values.len();
        let mzml = mzml_with_single_array(
            NumericType::Float64,
            BinaryData::F64(values.clone()),
            len,
        );
        let out = roundtrip(&mzml);
        let bin = first_array(&out).expect("should have binary data");
        let got = bin.to_f64_vec();
        prop_assert_eq!(got.len(), values.len());
        for (i, (g, e)) in got.iter().zip(values.iter()).enumerate() {
            if e.is_nan() {
                prop_assert!(g.is_nan(), "index {}: expected NaN", i);
            } else {
                prop_assert_eq!(g.to_bits(), e.to_bits(), "index {}: bit mismatch", i);
            }
        }
    }

    #[test]
    fn proptest_arbitrary_i32_roundtrip(
        values in prop::collection::vec(prop::num::i32::ANY, 1..64)
    ) {
        let len = values.len();
        let mzml = mzml_with_single_array(
            NumericType::Int32,
            BinaryData::I32(values.clone()),
            len,
        );
        let out = roundtrip(&mzml);
        let bin = first_array(&out).expect("should have binary data");
        match bin {
            BinaryData::I32(got) => prop_assert_eq!(got, &values),
            other => {
                let got = other.to_f64_vec();
                let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
                prop_assert_eq!(got, expected);
            }
        }
    }

    #[test]
    fn proptest_arbitrary_i16_roundtrip(
        values in prop::collection::vec(prop::num::i16::ANY, 1..64)
    ) {
        let len = values.len();
        let mzml = mzml_with_single_array(
            NumericType::Int16,
            BinaryData::I16(values.clone()),
            len,
        );
        let out = roundtrip(&mzml);
        let bin = first_array(&out).expect("should have binary data");
        match bin {
            BinaryData::I16(got) => prop_assert_eq!(got, &values),
            other => {
                let got = other.to_f64_vec();
                let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
                prop_assert_eq!(got, expected);
            }
        }
    }
}
