mod common;

use common::assertions::*;
use common::builders::*;
use common::fixtures;
use common::BinaryDataExt;
use ionic::mzml::structs::*;

// Tests 19-22: Rare numeric types, XML roundtrip without array length,
// parser honors shorter array length, preserves integer ms level arrays.

#[test]
fn ion_roundtrip_preserves_rare_numeric_types() {
    let cases = [
        (
            NumericType::Float16,
            BinaryData::F16(vec![0x0000, 0x3c00, 0x4000]),
            BinaryData::F16(vec![0x0000, 0x3555, 0x3c00]),
        ),
        (
            NumericType::Int16,
            BinaryData::I16(vec![-10, 0, 10]),
            BinaryData::I16(vec![-20, 0, 20]),
        ),
        (
            NumericType::Int32,
            BinaryData::I32(vec![-1_000, 0, 1_000]),
            BinaryData::I32(vec![-2_000, 0, 2_000]),
        ),
        (
            NumericType::Int64,
            BinaryData::I64(vec![-1_000_000, 0, 1_000_000]),
            BinaryData::I64(vec![-2_000_000, 0, 2_000_000]),
        ),
    ];

    for (numeric_type, spectrum_binary, chromatogram_binary) in cases {
        let src = synthetic_numeric_matrix_mzml(
            numeric_type,
            spectrum_binary,
            chromatogram_binary,
            Some(3),
        );
        let out = common::decode_ion(&common::encode_to_ion(&src, 9, false))
            .expect("decode should succeed");
        assert_mzml_semantic_eq(&src, &out);
    }
}

#[test]
fn xml_roundtrip_preserves_rare_numeric_types_without_array_length() {
    let cases = [
        (
            NumericType::Float16,
            BinaryData::F16(vec![0x0000, 0x3c00, 0x4000]),
            BinaryData::F16(vec![0x0000, 0x3555, 0x3c00]),
        ),
        (
            NumericType::Int16,
            BinaryData::I16(vec![-10, 0, 10]),
            BinaryData::I16(vec![-20, 0, 20]),
        ),
        (
            NumericType::Int32,
            BinaryData::I32(vec![-1_000, 0, 1_000]),
            BinaryData::I32(vec![-2_000, 0, 2_000]),
        ),
        (
            NumericType::Int64,
            BinaryData::I64(vec![-1_000_000, 0, 1_000_000]),
            BinaryData::I64(vec![-2_000_000, 0, 2_000_000]),
        ),
    ];

    for (numeric_type, spectrum_binary, chromatogram_binary) in cases {
        let src =
            synthetic_numeric_matrix_mzml(numeric_type, spectrum_binary, chromatogram_binary, None);
        let xml = ionic::mzml::bin_to_mzml::bin_to_mzml(&src).expect("bin_to_mzml should succeed");
        let reparsed = common::parse_xml(&xml);
        assert_mzml_semantic_eq(&src, &reparsed);
    }
}

#[test]
fn parser_honors_declared_shorter_array_length() {
    let cases = [
        (
            NumericType::Float16,
            BinaryData::F16(vec![0x0000, 0x3c00, 0x4000]),
        ),
        (NumericType::Int16, BinaryData::I16(vec![-10, 0, 10])),
        (NumericType::Int32, BinaryData::I32(vec![-1_000, 0, 1_000])),
        (
            NumericType::Int64,
            BinaryData::I64(vec![-1_000_000, 0, 1_000_000]),
        ),
    ];

    for (numeric_type, binary) in cases {
        let xml = single_array_xml("MS:1000514", numeric_type, &binary, Some(2));
        let mzml = common::parse_xml(&xml);
        let spectrum = common::spectrum_by_id(&mzml, "scan=1");
        let array = &common::spectrum_arrays(spectrum)[0];

        assert_eq!(array.numeric_type, Some(numeric_type));
        assert_eq!(array.array_length, Some(2));
        assert_eq!(
            array.binary.as_ref().expect("decoded binary present").len(),
            2,
            "parser should truncate payload to declared arrayLength for {numeric_type:?}"
        );
    }
}

#[test]
fn encoder_preserves_integer_ms_level_arrays() {
    let src = fixtures::anpc_test_mzml();
    let bytes = common::encode_to_ion(src, 10, false);
    let out = common::decode_ion(&bytes).expect("decode should succeed");

    let tic = common::chromatogram_by_id(&out, "TIC");
    let arrays = common::chromatogram_arrays(tic);
    let ms_level = common::find_array_by_accession(arrays, "MS:1000786");
    assert_eq!(ms_level.numeric_type, Some(NumericType::Int64));

    match ms_level.binary.as_ref().expect("ms level binary present") {
        BinaryData::I64(v) => {
            assert!(!v.is_empty(), "ms level array must be non-empty");
        }
        other => panic!("ms level array must be I64, got {other:?}"),
    }
}
