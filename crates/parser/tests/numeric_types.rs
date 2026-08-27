mod common;

use common::{assertions::*, helpers, helpers::*};
use ionic::mzml::structs::*;
use std::borrow::Cow;

#[test]
fn ion_roundtrip_preserves_rare_numeric_types() {
    let cases = [
        (
            NumericType::Float16,
            NumericArray::F16(vec![0x0000, 0x3c00, 0x4000]),
            NumericArray::F16(vec![0x0000, 0x3555, 0x3c00]),
        ),
        (
            NumericType::Int16,
            NumericArray::I16(vec![-10, 0, 10]),
            NumericArray::I16(vec![-20, 0, 20]),
        ),
        (
            NumericType::Int32,
            NumericArray::I32(vec![-1_000, 0, 1_000]),
            NumericArray::I32(vec![-2_000, 0, 2_000]),
        ),
        (
            NumericType::Int64,
            NumericArray::I64(vec![-1_000_000, 0, 1_000_000]),
            NumericArray::I64(vec![-2_000_000, 0, 2_000_000]),
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
            NumericArray::F16(vec![0x0000, 0x3c00, 0x4000]),
            NumericArray::F16(vec![0x0000, 0x3555, 0x3c00]),
        ),
        (
            NumericType::Int16,
            NumericArray::I16(vec![-10, 0, 10]),
            NumericArray::I16(vec![-20, 0, 20]),
        ),
        (
            NumericType::Int32,
            NumericArray::I32(vec![-1_000, 0, 1_000]),
            NumericArray::I32(vec![-2_000, 0, 2_000]),
        ),
        (
            NumericType::Int64,
            NumericArray::I64(vec![-1_000_000, 0, 1_000_000]),
            NumericArray::I64(vec![-2_000_000, 0, 2_000_000]),
        ),
    ];

    for (numeric_type, spectrum_binary, chromatogram_binary) in cases {
        let src =
            synthetic_numeric_matrix_mzml(numeric_type, spectrum_binary, chromatogram_binary, None);
        let xml = ionic::mzml::bin_to_mzml::bin_to_mzml(&src).expect("bin_to_mzml should succeed");
        let reparsed = ionic::mzml::parse_mzml::parse_mzml(&xml).expect("reparse should succeed");
        assert_mzml_semantic_eq(&src, &reparsed);
    }
}

#[test]
fn parser_honors_declared_shorter_array_length() {
    let cases = [
        (
            NumericType::Float16,
            NumericArray::F16(vec![0x0000, 0x3c00, 0x4000]),
        ),
        (NumericType::Int16, NumericArray::I16(vec![-10, 0, 10])),
        (
            NumericType::Int32,
            NumericArray::I32(vec![-1_000, 0, 1_000]),
        ),
        (
            NumericType::Int64,
            NumericArray::I64(vec![-1_000_000, 0, 1_000_000]),
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
    let src = MzML {
        cv_list: Some(helpers::default_cv_list_like_writer()),
        file_description: Some(helpers::minimal_file_description()),
        run: Run {
            id: "int-array-test".to_string(),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: Some(3),
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(2),
                        binary_data_arrays: vec![
                            helpers::synthetic_binary_data_array(
                                "MS:1000514",
                                NumericType::Float64,
                                NumericArray::F64(vec![100.0, 200.0, 300.0]),
                                Some(3),
                            ),
                            helpers::synthetic_binary_data_array(
                                "MS:1000515",
                                NumericType::Float64,
                                NumericArray::F64(vec![1.0, 2.0, 3.0]),
                                Some(3),
                            ),
                        ],
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            chromatogram_list: Some(ChromatogramList {
                count: Some(1),
                chromatograms: vec![Chromatogram {
                    id: "TIC".to_string(),
                    index: Some(0),
                    default_array_length: Some(3),
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(3),
                        binary_data_arrays: vec![
                            helpers::synthetic_binary_data_array(
                                "MS:1000595",
                                NumericType::Float64,
                                NumericArray::F64(vec![1.0, 2.0, 3.0]),
                                Some(3),
                            ),
                            helpers::synthetic_binary_data_array(
                                "MS:1000515",
                                NumericType::Float64,
                                NumericArray::F64(vec![10.0, 20.0, 30.0]),
                                Some(3),
                            ),
                            BinaryDataArray {
                                array_length: Some(3),
                                cv_params: vec![
                                    CvParam {
                                        cv_ref: Some(Cow::Borrowed("MS")),
                                        accession: Some(Cow::Borrowed("MS:1000786")),
                                        name: Cow::Borrowed("ms level array"),
                                        ..Default::default()
                                    },
                                    helpers::synthetic_ms_cv(
                                        helpers::precision_accession(NumericType::Int64),
                                        None,
                                    ),
                                    helpers::synthetic_ms_cv("MS:1000576", None),
                                ],
                                numeric_type: Some(NumericType::Int64),
                                binary: Some(NumericArray::I64(vec![1, 1, 2])),
                                ..Default::default()
                            },
                        ],
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let bytes = common::encode_to_ion(&src, 10, false);
    let out = common::decode_ion(&bytes).expect("decode should succeed");

    let tic = common::chromatogram_by_id(&out, "TIC");
    let arrays = common::chromatogram_arrays(tic);
    let ms_level = common::find_array_by_accession(arrays, "MS:1000786");
    assert_eq!(ms_level.numeric_type, Some(NumericType::Int64));

    match ms_level.binary.as_ref().expect("ms level binary present") {
        NumericArray::I64(v) => {
            assert!(!v.is_empty(), "ms level array must be non-empty");
        }
        other => panic!("ms level array must be I64, got {other:?}"),
    }
}
