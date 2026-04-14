mod common;

use common::helpers::{minimal_file_description, synthetic_binary_data_array};
use ionic::ion::{Decoder, DecoderConfig, WritingMode, encode};
use ionic::mzml::structs::*;

#[test]
fn memory_mode_roundtrip_multi_spectrum() {
    let mzml = build_multi_spectrum_mzml(10, 100);

    let mut buf = Vec::new();
    encode(&mzml, 6, false, WritingMode::Memory, &mut buf).expect("encode should succeed");
    assert!(!buf.is_empty(), "output should not be empty");

    let mut decoder = Decoder::open(&buf, DecoderConfig::default()).expect("decoder open");
    let decoded = decoder.to_mzml().expect("to_mzml");

    let orig_spectra = common::spectra(&mzml);
    let dec_spectra = common::spectra(&decoded);
    assert_eq!(orig_spectra.len(), dec_spectra.len());

    for idx in [0, orig_spectra.len() - 1] {
        let orig_mz = common::first_array_values_by_accession(&orig_spectra[idx], "MS:1000514");
        let dec_mz = common::first_array_values_by_accession(&dec_spectra[idx], "MS:1000514");
        assert_eq!(
            orig_mz, dec_mz,
            "m/z values differ for spectrum index {idx}"
        );
    }
}

#[test]
fn streaming_mode_roundtrip_via_tempfile() {
    use ionic::ion::encoder::FileEncoderOutput;
    use std::fs;

    let mzml = build_multi_spectrum_mzml(5, 50);
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("ionic_test_streaming.ion");

    let mut file_output = FileEncoderOutput::open_for_writing(temp_path.to_str().unwrap())
        .unwrap_or_else(|e| {
            panic!("failed to create FileEncoderOutput: {e}");
        });

    encode(&mzml, 3, false, WritingMode::Streaming, &mut file_output)
        .expect("streaming encode should succeed");
    drop(file_output);

    let bytes = fs::read(&temp_path).expect("should read temp file");
    assert!(!bytes.is_empty(), "file should not be empty");

    let mut decoder = Decoder::open(&bytes, DecoderConfig::default()).expect("decoder open");
    let decoded = decoder.to_mzml().expect("to_mzml");

    assert_eq!(
        common::spectra(&mzml).len(),
        common::spectra(&decoded).len(),
        "spectrum count mismatch"
    );

    let _ = fs::remove_file(&temp_path);
}

#[test]
fn memory_and_streaming_produce_equivalent_results() {
    use ionic::ion::encoder::FileEncoderOutput;
    use std::fs;

    let mzml = build_multi_spectrum_mzml(3, 20);

    let mut mem_buf = Vec::new();
    encode(&mzml, 0, false, WritingMode::Memory, &mut mem_buf).expect("memory encode");

    let temp_path = std::env::temp_dir().join("ionic_test_equiv.ion");
    let mut file_output = FileEncoderOutput::open_for_writing(temp_path.to_str().unwrap())
        .expect("create file output");
    encode(&mzml, 0, false, WritingMode::Streaming, &mut file_output).expect("streaming encode");
    drop(file_output); // ensure flush
    let stream_buf = fs::read(&temp_path).expect("read temp file");

    let mut mem_decoder = Decoder::open(&mem_buf, DecoderConfig::default()).expect("mem decoder");
    let mem_decoded = mem_decoder.to_mzml().expect("mem to_mzml");

    let mut stream_decoder =
        Decoder::open(&stream_buf, DecoderConfig::default()).expect("stream decoder");
    let stream_decoded = stream_decoder.to_mzml().expect("stream to_mzml");

    let diffs = common::canonical_diff_paths(&mem_decoded, &stream_decoded);
    assert!(
        diffs.is_empty(),
        "memory vs streaming decode differ:\n{}",
        diffs.join("\n")
    );

    let _ = fs::remove_file(&temp_path);
}

#[test]
fn large_array_roundtrip_stress() {
    let n = 10_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.01).collect();
    let intensity: Vec<f64> = (0..n).map(|i| (i as f64).sin().abs() * 1e6).collect();

    let mzml = MzML {
        file_description: Some(minimal_file_description()),
        run: Run {
            id: "large-test".to_string(),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: Some(n),
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(2),
                        binary_data_arrays: vec![
                            synthetic_binary_data_array(
                                "MS:1000514",
                                NumericType::Float64,
                                BinaryData::F64(mz.clone()),
                                Some(n),
                            ),
                            synthetic_binary_data_array(
                                "MS:1000515",
                                NumericType::Float64,
                                BinaryData::F64(intensity.clone()),
                                Some(n),
                            ),
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

    let mut buf = Vec::new();
    encode(&mzml, 12, false, WritingMode::Memory, &mut buf).expect("encode large array");

    let mut decoder = Decoder::open(&buf, DecoderConfig::default()).expect("decoder open");
    let decoded = decoder.to_mzml().expect("to_mzml");

    let dec_spectra = common::spectra(&decoded);
    assert_eq!(dec_spectra.len(), 1);

    let dec_mz = common::first_array_values_by_accession(&dec_spectra[0], "MS:1000514");
    let dec_int = common::first_array_values_by_accession(&dec_spectra[0], "MS:1000515");
    assert_eq!(dec_mz, mz);
    assert_eq!(dec_int, intensity);
}

fn build_multi_spectrum_mzml(n_spectra: usize, n_points: usize) -> MzML {
    let spectra: Vec<Spectrum> = (0..n_spectra)
        .map(|i| {
            let mz: Vec<f64> = (0..n_points)
                .map(|j| 100.0 + j as f64 + i as f64 * 0.1)
                .collect();
            let intensity: Vec<f64> = (0..n_points).map(|j| (j as f64 + 1.0) * 100.0).collect();
            Spectrum {
                id: format!("scan={}", i + 1),
                index: Some(i as u32),
                default_array_length: Some(n_points),
                binary_data_array_list: Some(BinaryDataArrayList {
                    count: Some(2),
                    binary_data_arrays: vec![
                        synthetic_binary_data_array(
                            "MS:1000514",
                            NumericType::Float64,
                            BinaryData::F64(mz),
                            Some(n_points),
                        ),
                        synthetic_binary_data_array(
                            "MS:1000515",
                            NumericType::Float64,
                            BinaryData::F64(intensity),
                            Some(n_points),
                        ),
                    ],
                }),
                ..Default::default()
            }
        })
        .collect();

    MzML {
        file_description: Some(minimal_file_description()),
        run: Run {
            id: "file-test".to_string(),
            spectrum_list: Some(SpectrumList {
                count: Some(n_spectra),
                spectra,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}
