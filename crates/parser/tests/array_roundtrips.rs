mod common;

use common::{
    binary_ext::BinaryDataExt,
    decode_ion, encode_to_ion, first_spectrum_binary,
    helpers::{build_mzml, make_spectrum_f64, mzml_with_single_array},
    roundtrip,
};
use ionic::{
    ion::{IonReader, ReadOptions, WriteOptions, write_mzml_to_ion},
    mzml::structs::*,
};
use proptest::prelude::*;
use std::borrow::Cow;

#[test]
fn roundtrip_f64_array() {
    let values = vec![1.0_f64, -2.5, 0.0, f64::MAX, f64::MIN, std::f64::consts::PI];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Float64, NumericArray::F64(values.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("should have binary data");
    let got = bin.to_f64_vec();
    assert_eq!(got, values);
}

#[test]
fn roundtrip_f32_array() {
    let values = vec![1.0_f32, -2.5, 0.0, f32::MAX, f32::MIN, std::f32::consts::PI];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Float32, NumericArray::F32(values.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("should have binary data");
    let got = bin.to_f64_vec();
    let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
    assert_eq!(got, expected);
}

#[test]
fn array_filter_label_matches_applied_transform() {
    let raw_filter = 0u8;
    let f64_dtype = 1u8;
    let f32_dtype = 2u8;

    let f32_values = vec![10.0_f32, 11.0, 12.0, 13.0];
    let f32_len = f32_values.len();
    let f32_mzml =
        mzml_with_single_array(NumericType::Float32, NumericArray::F32(f32_values), f32_len);

    let f64_values = vec![100.0_f64, 100.5, 101.0, 101.5];
    let f64_len = f64_values.len();
    let f64_mzml =
        mzml_with_single_array(NumericType::Float64, NumericArray::F64(f64_values), f64_len);

    let f32_bytes = encode_to_ion(&f32_mzml, 9, false);
    let f32_decoder = IonReader::open(&f32_bytes, ReadOptions::default()).expect("open f32 ion");
    let f32_refs = f32_decoder
        .spectrum_array_addresses(0)
        .expect("f32 array refs");

    let f64_bytes = encode_to_ion(&f64_mzml, 9, false);
    let f64_decoder = IonReader::open(&f64_bytes, ReadOptions::default()).expect("open f64 ion");
    let f64_refs = f64_decoder
        .spectrum_array_addresses(0)
        .expect("f64 array refs");

    assert!(
        f32_refs
            .iter()
            .any(|a| a.dtype() == f32_dtype && a.array_filter() == raw_filter),
        "f32 intensity must be tagged raw"
    );
    assert!(
        f64_refs
            .iter()
            .any(|a| a.dtype() == f64_dtype && a.array_filter() == raw_filter),
        "f64 intensity must be tagged raw (intensity is never delta-shuffled)"
    );
}

#[test]
fn roundtrip_i64_array() {
    let values = vec![0_i64, 1, -1, i64::MAX, i64::MIN, 42];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Int64, NumericArray::I64(values.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("should have binary data");
    match bin {
        NumericArray::I64(got) => assert_eq!(got, &values),
        other => {
            let got = other.to_f64_vec();
            let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
            assert_eq!(
                got, expected,
                "i64 roundtrip values differ (via f64 conversion)"
            );
        }
    }
}

#[test]
fn roundtrip_i32_array() {
    let values = vec![0_i32, 1, -1, i32::MAX, i32::MIN, 42];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Int32, NumericArray::I32(values.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("should have binary data");
    match bin {
        NumericArray::I32(got) => assert_eq!(got, &values),
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
    let mzml = mzml_with_single_array(NumericType::Int16, NumericArray::I16(values.clone()), len);
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("should have binary data");
    match bin {
        NumericArray::I16(got) => assert_eq!(got, &values),
        other => {
            let got = other.to_f64_vec();
            let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
            assert_eq!(got, expected, "i16 roundtrip values differ");
        }
    }
}

#[test]
fn roundtrip_single_element_per_type() {
    let cases: Vec<(NumericType, NumericArray)> = vec![
        (NumericType::Float64, NumericArray::F64(vec![42.0])),
        (NumericType::Float32, NumericArray::F32(vec![42.0])),
        (NumericType::Int64, NumericArray::I64(vec![42])),
        (NumericType::Int32, NumericArray::I32(vec![42])),
        (NumericType::Int16, NumericArray::I16(vec![42])),
    ];

    for (nt, bin) in cases {
        let mzml = mzml_with_single_array(nt, bin.clone(), 1);
        let out = roundtrip(&mzml);
        let got = first_spectrum_binary(&out).expect("should have binary data");
        let expected_f64 = bin.to_f64_vec();
        let got_f64 = got.to_f64_vec();
        assert_eq!(
            got_f64, expected_f64,
            "single-element roundtrip failed for {nt:?}"
        );
    }
}
#[test]
fn roundtrip_at_compression_levels() {
    let values = vec![100.0_f64, 200.0, 300.0, 400.0, 500.0];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Float64, NumericArray::F64(values.clone()), len);

    for level in [0, 3, 10, 22] {
        let mut buf = Vec::new();
        write_mzml_to_ion(
            &mzml,
            WriteOptions {
                compression_level: level,
                force_f32: false,
                ..Default::default()
            },
            &mut buf,
        )
        .unwrap_or_else(|e| panic!("encode at level {level} failed: {e}"));
        let mut decoder = IonReader::open(&buf, ReadOptions::default())
            .unwrap_or_else(|e| panic!("decoder open at level {level} failed: {e}"));
        let decoded = decoder
            .to_mzml()
            .unwrap_or_else(|e| panic!("to_mzml at level {level} failed: {e}"));
        let got = first_spectrum_binary(&decoded).expect("should have binary data");
        assert_eq!(
            got.to_f64_vec(),
            values,
            "values differ at compression level {level}"
        );
    }
}
#[test]
fn roundtrip_force_f32_downcasts() {
    let values = vec![100.0_f64, 200.0, 300.0];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Float64, NumericArray::F64(values.clone()), len);

    let mut buf = Vec::new();
    write_mzml_to_ion(
        &mzml,
        WriteOptions {
            compression_level: 0,
            force_f32: true,
            ..Default::default()
        },
        &mut buf,
    )
    .expect("encode with force_f32");
    let mut decoder = IonReader::open(&buf, ReadOptions::default()).expect("decoder open");
    let decoded = decoder.to_mzml().expect("to_mzml");

    let bin = first_spectrum_binary(&decoded).expect("should have binary data");
    let got = bin.to_f64_vec();
    for (i, (g, e)) in got.iter().zip(values.iter()).enumerate() {
        let expected_f32 = *e as f32 as f64;
        assert!(
            (g - expected_f32).abs() < 1e-6,
            "index {i}: force_f32 value mismatch: {g} vs {expected_f32}"
        );
    }
}

fn encode_decode_mz_compressed(mz: Vec<f64>) -> Vec<f64> {
    let len = mz.len();
    let mzml = mzml_with_single_array(NumericType::Float64, NumericArray::F64(mz), len);
    let buf = encode_to_ion(&mzml, 3, false);
    let decoded = decode_ion(&buf).unwrap();
    first_spectrum_binary(&decoded).unwrap().to_f64_vec()
}

#[test]
fn delta_mz_single_element_is_bit_exact() {
    let input = vec![503.42f64];
    let got = encode_decode_mz_compressed(input.clone());
    assert_eq!(got[0].to_bits(), input[0].to_bits());
}

#[test]
fn delta_mz_two_elements_are_bit_exact() {
    let input = vec![100.0f64, 200.5];
    let got = encode_decode_mz_compressed(input.clone());
    for (a, b) in got.iter().zip(input.iter()) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn delta_mz_monotonic_array_is_bit_exact() {
    let input: Vec<f64> = (0..10_000).map(|i| 100.0 + i as f64 * 0.01).collect();
    let got = encode_decode_mz_compressed(input.clone());
    for (a, b) in got.iter().zip(input.iter()) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn delta_mz_special_values_are_bit_exact() {
    let input = vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0f64, 0.0f64];
    let got = encode_decode_mz_compressed(input.clone());
    for (a, b) in got.iter().zip(input.iter()) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn delta_mz_via_for_each_scan_is_bit_exact() {
    use ionic::{IonReader, ScanSource};
    let input: Vec<f64> = (0..500).map(|i| 100.0 + i as f64 * 0.05).collect();
    let intensity: Vec<f64> = vec![1.0; input.len()];
    let mut spectrum = make_spectrum_f64("scan=1", input.clone(), intensity);
    spectrum.scan_list = Some(ScanList {
        count: Some(1),
        scans: vec![Scan {
            cv_params: vec![CvParam {
                cv_ref: Some(Cow::Borrowed("MS")),
                accession: Some(Cow::Borrowed("MS:1000016")),
                name: Cow::Borrowed("scan start time"),
                value: Some(Cow::Borrowed("1.0")),
                unit_accession: Some(Cow::Borrowed("UO:0000031")),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    });
    let mzml = build_mzml(vec![spectrum], vec![]);
    let buf = encode_to_ion(&mzml, 3, false);
    let mut ion = IonReader::open(&buf, ReadOptions::default()).unwrap();
    let mut got_mz: Vec<f64> = Vec::new();
    ion.for_each_in_range(0.0, f64::MAX, 0, |_, mz: &[f64], _| {
        got_mz = mz.to_vec();
    });
    assert_eq!(got_mz.len(), input.len());
    for (a, b) in got_mz.iter().zip(input.iter()) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn delta_on_mz_raw_on_intensity() {
    use ionic::ion::IonReader as IonDecoder;
    let mz: Vec<f64> = (0..100).map(|i| 100.0 + i as f64).collect();
    let intensity: Vec<f64> = (0..100).map(|i| (i * 10) as f64).collect();
    let mzml = build_mzml(
        vec![make_spectrum_f64("scan=1", mz.clone(), intensity.clone())],
        vec![],
    );
    let buf = encode_to_ion(&mzml, 3, false);
    let mut decoder = IonDecoder::open(&buf, ReadOptions::default()).unwrap();
    let refs = decoder.spectrum_array_addresses(0).unwrap();
    let mz_address = refs.iter().find(|r| r.array_type() == 1_000_514).unwrap();
    let int_ref = refs.iter().find(|r| r.array_type() == 1_000_515).unwrap();
    assert_eq!(
        mz_address.array_filter(),
        2,
        "m/z must use DeltaShuffle filter"
    );
    assert_eq!(
        int_ref.array_filter(),
        0,
        "intensity must use raw filter (intensity is never delta-shuffled)"
    );
    let mut got_mz = Vec::new();
    decoder
        .read_spectrum_values(mz_address, &mut got_mz)
        .unwrap();
    let mut got_int = Vec::new();
    decoder.read_spectrum_values(int_ref, &mut got_int).unwrap();
    for (a, b) in got_mz.iter().zip(mz.iter()) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
    for (a, b) in got_int.iter().zip(intensity.iter()) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn format_version_always_matches_current() {
    use ionic::ion::{CURRENT_VERSION, HEADER_FORMAT_VERSION_OFFSET};
    let values = vec![1.0_f64, 2.0, 3.0, 4.0, 5.0];
    let len = values.len();
    let mzml = mzml_with_single_array(NumericType::Float64, NumericArray::F64(values.clone()), len);
    for level in [0u8, 3, 22] {
        let buf = encode_to_ion(&mzml, level, false);
        let format_version = u16::from_le_bytes(
            buf[HEADER_FORMAT_VERSION_OFFSET..HEADER_FORMAT_VERSION_OFFSET + 2]
                .try_into()
                .unwrap(),
        );
        assert_eq!(
            format_version, CURRENT_VERSION,
            "format_version must match CURRENT_VERSION at compression level {level}"
        );
    }
    let out = roundtrip(&mzml);
    let bin = first_spectrum_binary(&out).expect("binary data");
    for (a, b) in bin.to_f64_vec().iter().zip(values.iter()) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn delta_not_applied_without_compression() {
    use ionic::ion::IonReader as IonDecoder;
    let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
    let intensity = vec![1.0_f64; mz.len()];
    let mzml = build_mzml(
        vec![make_spectrum_f64("scan=1", mz.clone(), intensity)],
        vec![],
    );
    let buf = encode_to_ion(&mzml, 0, false);
    let mut decoder = IonDecoder::open(&buf, ReadOptions::default()).unwrap();
    let refs = decoder.spectrum_array_addresses(0).unwrap();
    let mz_address = refs.iter().find(|r| r.array_type() == 1_000_514).unwrap();
    assert_eq!(mz_address.array_filter(), 0, "no delta without compression");
    let mut got = Vec::new();
    decoder.read_spectrum_values(mz_address, &mut got).unwrap();
    for (a, b) in got.iter().zip(mz.iter()) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn delta_shuffle_applied_to_time_array() {
    use common::helpers::make_chromatogram_f64;
    use ionic::ion::IonReader as IonDecoder;
    let time: Vec<f64> = (0..100).map(|i| i as f64 * 0.1).collect();
    let intensity: Vec<f64> = vec![1.0; time.len()];
    let mzml = build_mzml(
        vec![],
        vec![make_chromatogram_f64("tic", time.clone(), intensity)],
    );
    let buf = encode_to_ion(&mzml, 3, false);
    let mut decoder = IonDecoder::open(&buf, ReadOptions::default()).unwrap();
    let refs = decoder.chromatogram_array_addresses(0).unwrap();
    let time_ref = refs.iter().find(|r| r.array_type() == 1_000_595).unwrap();
    assert_eq!(
        time_ref.array_filter(),
        2,
        "time must use DeltaShuffle filter"
    );
    let mut got = Vec::new();
    decoder
        .read_chromatogram_values(time_ref, &mut got)
        .unwrap();
    for (a, b) in got.iter().zip(time.iter()) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn read_chromatogram_array_keeps_native_f32_width_33() {
    use common::helpers::make_chromatogram_f64;
    let time: Vec<f64> = (0..64).map(|i| i as f64 * 0.5).collect();
    let intensity: Vec<f64> = (0..64).map(|i| i as f64).collect();
    let mzml = build_mzml(vec![], vec![make_chromatogram_f64("tic", time, intensity)]);
    let buf = encode_to_ion(&mzml, 3, true);
    let mut decoder = IonReader::open(&buf, ReadOptions::default()).unwrap();
    let refs = decoder.chromatogram_array_addresses(0).unwrap();
    let intensity_ref = refs.iter().find(|r| r.array_type() == 1_000_515).unwrap();
    let native = decoder.read_chromatogram_array(intensity_ref).unwrap();
    assert!(
        matches!(native, NumericArray::F32(_)),
        "read_chromatogram_array must keep the stored native f32 width"
    );
    let mut widened = Vec::new();
    decoder
        .read_chromatogram_values(intensity_ref, &mut widened)
        .unwrap();
    assert_eq!(
        widened.len(),
        64,
        "read_chromatogram_values must still widen to f64"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn proptest_arbitrary_f64_roundtrip(
        values in prop::collection::vec(prop::num::f64::ANY, 1..64)
    ) {
        let len = values.len();
        let mzml = mzml_with_single_array(
            NumericType::Float64,
            NumericArray::F64(values.clone()),
            len,
        );
        let out = roundtrip(&mzml);
        let bin = first_spectrum_binary(&out).expect("should have binary data");
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
    fn proptest_delta2vbyte_mz_roundtrip(
        mz in prop::collection::vec(0.0f64..100_000.0, 3..256)
    ) {
        let mut sorted_mz = mz.clone();
        sorted_mz.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let intensity = vec![1.0f64; sorted_mz.len()];
        let mzml = build_mzml(
            vec![make_spectrum_f64("scan=1", sorted_mz.clone(), intensity)],
            vec![],
        );
        let buf = encode_to_ion(&mzml, 3, false);
        let got = decode_ion(&buf).unwrap();
        let bin = first_spectrum_binary(&got).expect("should have binary data");
        let decoded = bin.to_f64_vec();
        prop_assert_eq!(decoded.len(), sorted_mz.len());
        for (i, (g, e)) in decoded.iter().zip(sorted_mz.iter()).enumerate() {
            if e.is_nan() {
                prop_assert!(g.is_nan(), "index {}: expected NaN", i);
            } else {
                prop_assert_eq!(g.to_bits(), e.to_bits(), "index {}: bit mismatch", i);
            }
        }
    }

    #[test]
    fn proptest_chimp_time_roundtrip(
        time in prop::collection::vec(prop::num::f64::ANY, 2..256)
    ) {
        use common::helpers::make_chromatogram_f64;
        use common::first_chrom_array_values_by_accession;
        let intensity = vec![1.0f64; time.len()];
        let mzml = build_mzml(
            vec![],
            vec![make_chromatogram_f64("tic", time.clone(), intensity)],
        );
        let buf = encode_to_ion(&mzml, 3, false);
        let got = decode_ion(&buf).unwrap();
        let chrom_list = got.run.chromatogram_list.unwrap();
        let decoded = first_chrom_array_values_by_accession(&chrom_list.chromatograms[0], "MS:1000595");
        prop_assert_eq!(decoded.len(), time.len());
        for (i, (g, e)) in decoded.iter().zip(time.iter()).enumerate() {
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
            NumericArray::I32(values.clone()),
            len,
        );
        let out = roundtrip(&mzml);
        let bin = first_spectrum_binary(&out).expect("should have binary data");
        match bin {
            NumericArray::I32(got) => prop_assert_eq!(got, &values),
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
            NumericArray::I16(values.clone()),
            len,
        );
        let out = roundtrip(&mzml);
        let bin = first_spectrum_binary(&out).expect("should have binary data");
        match bin {
            NumericArray::I16(got) => prop_assert_eq!(got, &values),
            other => {
                let got = other.to_f64_vec();
                let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
                prop_assert_eq!(got, expected);
            }
        }
    }
}
