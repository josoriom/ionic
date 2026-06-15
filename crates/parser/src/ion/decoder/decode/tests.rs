use super::*;
use super::arrays::decode_into;
use crate::ion::encoder::utilities::SectionStorage;
use crate::mzml::structs::{NumericArray, BinaryDataArray, CvParam};

const BYTES: &[u8] = include_bytes!("../../../../data/ion/test.ion");

fn ref_with(array_type: u32, continues_previous_segment: u8, encoded_len: u32) -> ArrayRef {
    ArrayRef {
        block_id: 0,
        element_offset: 0,
        element_count: 4,
        array_type,
        dtype: FILE_DTYPE_F64,
        array_filter: 0,
        encoded_len,
        continues_previous_segment,
    }
}

#[test]
fn group_arrays_keeps_same_accession_logical_arrays_separate() {
    let refs = [ref_with(1000514, 0, 0), ref_with(1000514, 0, 0)];
    let groups = group_arrays(&refs).unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].refs.len(), 1);
    assert_eq!(groups[1].refs.len(), 1);
}

#[test]
fn group_arrays_joins_continuation_segments_into_one_group() {
    let refs = [ref_with(1000514, 0, 0), ref_with(1000514, 1, 0)];
    let groups = group_arrays(&refs).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].refs.len(), 2);
}

#[test]
fn group_arrays_errors_on_leading_continuation() {
    let refs = [ref_with(1000514, 1, 0)];
    assert!(group_arrays(&refs).is_err());
}

#[test]
fn group_arrays_errors_on_invalid_continues_value() {
    let refs = [ref_with(1000514, 0, 0), ref_with(1000514, 2, 0)];
    assert!(group_arrays(&refs).is_err());
}

#[test]
fn group_arrays_errors_on_multi_ref_variable_length() {
    let refs = [ref_with(1000514, 0, 8), ref_with(1000514, 1, 8)];
    assert!(group_arrays(&refs).is_err());
}

#[test]
fn group_arrays_errors_on_type_mismatch_in_continuation() {
    let mut second = ref_with(1000514, 1, 0);
    second.array_type = 1000515;
    let refs = [ref_with(1000514, 0, 0), second];
    assert!(group_arrays(&refs).is_err());
}

#[test]
fn new_reader_opens_old_fixture() {
    let reader = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    assert!(matches!(
        reader.spec_window_bounds,
        WindowBoundsCache::Unloaded
    ));
}

#[test]
fn get_spectrum_lazy_matches_full_conversion() {
    let mut reader = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    let full = reader.to_mzml().unwrap();
    let full_spectra = full.run.spectrum_list.expect("spectrum list").spectra;
    for (index, full_spectrum) in full_spectra.iter().enumerate() {
        let lazy = reader
            .spectrum(index)
            .unwrap()
            .expect("spectrum present");
        assert_eq!(
            format!("{lazy:?}"),
            format!("{full_spectrum:?}"),
            "spectrum {index} differs between lazy and full paths"
        );
    }
}

#[test]
fn metadata_at_matches_filtered_full_read() {
    let mut reader = IonReader::open(BYTES, ReadOptions::default()).unwrap();

    let all_spectra = reader.spectrum_metadata().unwrap();
    for index in 0..reader.spectrum_count() as usize {
        let one = reader.spectrum_metadata_at(index).unwrap();
        let expected: Vec<_> = all_spectra
            .iter()
            .filter(|row| row.item_index as usize == index)
            .cloned()
            .collect();
        assert_eq!(format!("{one:?}"), format!("{expected:?}"));
    }

    let all_chroms = reader.chromatogram_metadata().unwrap();
    for index in 0..reader.chromatogram_count() as usize {
        let one = reader.chromatogram_metadata_at(index).unwrap();
        let expected: Vec<_> = all_chroms
            .iter()
            .filter(|row| row.item_index as usize == index)
            .cloned()
            .collect();
        assert_eq!(format!("{one:?}"), format!("{expected:?}"));
    }
}

#[test]
fn open_parses_header() {
    let d = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    assert!(d.spectrum_count() > 0);
}

#[test]
fn summary_returns_none_out_of_bounds() {
    let d = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    assert!(d.spec_summary(d.spectrum_count() as usize).is_none());
}

#[test]
fn summary_has_valid_rt() {
    let d = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    let r = d.spec_summary(0).unwrap();
    assert!(r.rt.is_finite() && r.rt >= 0.0);
    assert!(r.ms_level >= 1);
}

#[test]
fn array_refs_contain_mz_and_intensity() {
    let d = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    let refs = d.spectrum_arrays(0).unwrap();
    assert!(refs.iter().any(|a| a.array_type == ACC_MZ));
    assert!(refs.iter().any(|a| a.array_type == ACC_INT));
}

#[test]
fn read_array_produces_mz_values() {
    let mut d = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    let refs = d.spectrum_arrays(0).unwrap();
    let mz_ref = refs.iter().find(|a| a.array_type == ACC_MZ).unwrap();

    let mut mz = Vec::new();
    d.read_array(mz_ref, &mut mz).unwrap();

    assert!(!mz.is_empty());
    assert!(mz.iter().all(|v| v.is_finite()));
}

#[test]
fn for_each_scan_yields_matching_scans() {
    let mut d = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    let mut count = 0usize;
    d.for_each_in_range(0.0, f64::MAX, 0, |summary, mz, int| {
        assert!(summary.rt.is_finite());
        assert!(!mz.is_empty());
        assert_eq!(mz.len(), int.len());
        count += 1;
    });
    assert_eq!(count, d.spectrum_count() as usize);
}

#[test]
fn for_each_scan_filters_by_ms_level() {
    let mut d = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    let mut count = 0usize;
    d.for_each_in_range(0.0, f64::MAX, 1, |_, _, _| {
        count += 1;
    });
    let expected = (0..d.spectrum_count() as usize)
        .filter(|&i| d.spec_summary(i).is_some_and(|r| r.ms_level == 1))
        .count();
    assert_eq!(count, expected);
}

#[test]
fn scans_in_visits_every_scan_and_matches_read_window() {
    let mut reader = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    let mz = Range { from: 0.0, to: f64::MAX };
    let mut seen: Vec<(usize, Vec<f64>, Vec<f64>)> = Vec::new();
    reader
        .scans_in(mz, Select::All, None, &mut |window| {
            seen.push((window.index, window.mz.to_vec(), window.intensity.to_vec()));
        })
        .unwrap();
    assert_eq!(seen.len(), reader.spectrum_count() as usize);
    for (index, mz_values, intensity_values) in &seen {
        let direct = reader.read_window(*index, mz).unwrap();
        assert_eq!(*mz_values, direct.x.to_f64());
        assert_eq!(*intensity_values, direct.y.to_f64());
    }
}

#[test]
fn scans_in_ms_level_filter_selects_only_matching_scans() {
    let mut reader = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    let mz = Range { from: 0.0, to: f64::MAX };
    let mut count = 0usize;
    reader
        .scans_in(mz, Select::All, Some(1), &mut |_window| {
            count += 1;
        })
        .unwrap();
    let expected = (0..reader.spectrum_count() as usize)
        .filter(|&index| reader.spec_summary(index).is_some_and(|s| s.ms_level == 1))
        .count();
    assert_eq!(count, expected);
}

#[test]
fn to_mzml_produces_valid_structure() {
    let mut d = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    let mzml = d.to_mzml().unwrap();
    let sl = mzml.run.spectrum_list.as_ref().unwrap();
    assert!(!sl.spectra.is_empty());
    assert!(
        sl.spectra[0]
            .binary_data_array_list
            .as_ref()
            .unwrap()
            .binary_data_arrays
            .iter()
            .any(|b| b.binary.is_some())
    );
}

#[test]
fn global_metadata_returns_entries() {
    let d = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    assert!(!d.global_metadata().unwrap().is_empty());
}

#[test]
fn spectrum_metadata_returns_entries() {
    let d = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    assert!(!d.spectrum_metadata().unwrap().is_empty());
}

#[test]
fn custom_config_opens_successfully() {
    let config = ReadOptions {
        max_cached_bytes: 1024 * 1024,
        verify_checksums: true,
        parallel: true,
        decompression_limit: DecompressionLimit::default(),
    };
    let d = IonReader::open(BYTES, config).unwrap();
    assert!(d.spectrum_count() > 0);
}

#[test]
fn decode_into_f64_roundtrips() {
    let vals = [1.5f64, 2.5, 3.5];
    let raw: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut buf = Vec::new();
    decode_into(&mut buf, &raw, 1, 0).unwrap();
    assert_eq!(buf, vals);
}

#[test]
fn decode_into_f32_converts() {
    let vals = [1.0f32, 2.0];
    let raw: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut buf = Vec::new();
    decode_into(&mut buf, &raw, 2, 0).unwrap();
    assert!((buf[0] - 1.0).abs() < f64::EPSILON);
    assert!((buf[1] - 2.0).abs() < f64::EPSILON);
}

#[test]
fn dtype_stride_maps_all_types() {
    assert_eq!(dtype_stride(1), 8);
    assert_eq!(dtype_stride(2), 4);
    assert_eq!(dtype_stride(3), 2);
    assert_eq!(dtype_stride(4), 2);
    assert_eq!(dtype_stride(5), 4);
    assert_eq!(dtype_stride(6), 8);
}

#[test]
fn open_bytes_gives_same_result_as_open() {
    let bytes_arc: Arc<[u8]> = Arc::from(BYTES);
    let mut d1 = IonReader::open(BYTES, ReadOptions::default()).unwrap();
    let mut d2 = IonReader::open_bytes(bytes_arc, ReadOptions::default()).unwrap();
    assert_eq!(d1.spectrum_count(), d2.spectrum_count());
    let mzml1 = d1.to_mzml().unwrap();
    let mzml2 = d2.to_mzml().unwrap();
    assert_eq!(format!("{mzml1:?}"), format!("{mzml2:?}"));
}

#[test]
fn open_source_uses_provided_source() {
    use crate::ion::decoder::utilities::byte_source::{BytesSource, ReadBytes};
    let bytes_arc: Arc<[u8]> = Arc::from(BYTES);
    let source = Arc::new(BytesSource::new(bytes_arc.clone())) as Arc<dyn ReadBytes>;
    let mut d = IonReader::open_source(source, ReadOptions::default()).unwrap();
    assert!(d.spectrum_count() > 0);
    let mzml = d.to_mzml().unwrap();
    assert!(!mzml.run.spectrum_list.unwrap().spectra.is_empty());
}

#[test]
fn mixed_normal_and_oversized_spectra_preserve_order_and_data() {
    use crate::ion::encoder::{
        encode::{WriteOptions, TARGET_BLOCK_UNCOMPRESSED_BYTES},
        ion_writer::write_mzml_to_ion,
    };
    use crate::mzml::structs::{
        NumericArray, BinaryDataArray, BinaryDataArrayList, CvParam, MzML, Run, Spectrum,
        SpectrumList,
    };

    fn make_bda(accession: &str, name: &str, data: Vec<f64>) -> BinaryDataArray {
        BinaryDataArray {
            cv_params: vec![CvParam {
                cv_ref: Some("MS".to_string()),
                accession: Some(accession.to_string()),
                name: name.to_string(),
                value: None,
                unit_cv_ref: None,
                unit_name: None,
                unit_accession: None,
            }],
            binary: Some(NumericArray::F64(data)),
            ..Default::default()
        }
    }

    fn make_spectrum(id: &str, mz: Vec<f64>, int: Vec<f64>) -> Spectrum {
        Spectrum {
            id: id.to_string(),
            binary_data_array_list: Some(BinaryDataArrayList {
                count: Some(2),
                binary_data_arrays: vec![
                    make_bda("MS:1000514", "m/z array", mz),
                    make_bda("MS:1000515", "intensity array", int),
                ],
            }),
            ..Default::default()
        }
    }

    let small_count_before = 5;
    let small_count_after = 5;
    let huge_n = (TARGET_BLOCK_UNCOMPRESSED_BYTES / 8) * 2;

    let mut expected_spectra: Vec<(String, Vec<f64>, Vec<f64>)> = Vec::new();
    let mut spectra = Vec::new();

    for i in 0..small_count_before {
        let mz: Vec<f64> = (0..10).map(|j| 100.0 + i as f64 + j as f64 * 0.1).collect();
        let int: Vec<f64> = (0..10).map(|j| (i * 10 + j) as f64).collect();
        let id = format!("small_pre_{i}");
        spectra.push(make_spectrum(&id, mz.clone(), int.clone()));
        expected_spectra.push((id, mz, int));
    }

    let huge_mz: Vec<f64> = (0..huge_n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let huge_int: Vec<f64> = (0..huge_n).map(|i| i as f64 * 10.0).collect();
    let huge_id = "huge_ms1".to_string();
    spectra.push(make_spectrum(&huge_id, huge_mz.clone(), huge_int.clone()));
    expected_spectra.push((huge_id, huge_mz, huge_int));

    for i in 0..small_count_after {
        let mz: Vec<f64> = (0..10).map(|j| 500.0 + i as f64 + j as f64 * 0.1).collect();
        let int: Vec<f64> = (0..10).map(|j| (i * 20 + j) as f64).collect();
        let id = format!("small_post_{i}");
        spectra.push(make_spectrum(&id, mz.clone(), int.clone()));
        expected_spectra.push((id, mz, int));
    }

    let mzml_in = MzML {
        run: Run {
            spectrum_list: Some(SpectrumList {
                spectra,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut encoded = Vec::new();
    write_mzml_to_ion(
        &mzml_in,
        WriteOptions {
            compression_level: 3,
            force_f32: false,
            block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
            parallel: true,
            section_storage: SectionStorage::Memory,
            mz_window: 0.0,
        },
        &mut encoded,
    )
    .unwrap();

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    let mzml_out = decoder.to_mzml().unwrap();
    let out_spectra = mzml_out.run.spectrum_list.unwrap().spectra;

    assert_eq!(out_spectra.len(), expected_spectra.len());

    for (out, (expected_id, expected_mz, expected_int)) in
        out_spectra.iter().zip(expected_spectra.iter())
    {
        assert_eq!(&out.id, expected_id);
        let arrays = out
            .binary_data_array_list
            .as_ref()
            .unwrap()
            .binary_data_arrays
            .as_slice();
        let mz_out = arrays
            .iter()
            .find(|a| {
                a.cv_params
                    .iter()
                    .any(|cv| cv.accession.as_deref() == Some("MS:1000514"))
            })
            .and_then(|a| a.binary.as_ref())
            .unwrap();
        let int_out = arrays
            .iter()
            .find(|a| {
                a.cv_params
                    .iter()
                    .any(|cv| cv.accession.as_deref() == Some("MS:1000515"))
            })
            .and_then(|a| a.binary.as_ref())
            .unwrap();
        let NumericArray::F64(mz_vec) = mz_out else {
            panic!("expected F64 mz array for {expected_id}");
        };
        let NumericArray::F64(int_vec) = int_out else {
            panic!("expected F64 intensity array for {expected_id}");
        };
        assert_eq!(mz_vec, expected_mz, "mz mismatch for {expected_id}");
        assert_eq!(
            int_vec, expected_int,
            "intensity mismatch for {expected_id}"
        );
    }
}

#[test]
fn oversized_array_roundtrips_with_compression_and_parallel() {
    use crate::ion::encoder::{
        encode::{WriteOptions, TARGET_BLOCK_UNCOMPRESSED_BYTES},
        ion_writer::write_mzml_to_ion,
    };
    use crate::mzml::structs::{
        NumericArray, BinaryDataArray, BinaryDataArrayList, CvParam, MzML, Run, Spectrum,
        SpectrumList,
    };

    let n = (TARGET_BLOCK_UNCOMPRESSED_BYTES / 8) * 2;
    let mz_data: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int_data: Vec<f64> = (0..n).map(|i| i as f64 * 10.0).collect();

    fn make_bda(accession: &str, name: &str, data: Vec<f64>) -> BinaryDataArray {
        BinaryDataArray {
            cv_params: vec![CvParam {
                cv_ref: Some("MS".to_string()),
                accession: Some(accession.to_string()),
                name: name.to_string(),
                value: None,
                unit_cv_ref: None,
                unit_name: None,
                unit_accession: None,
            }],
            binary: Some(NumericArray::F64(data)),
            ..Default::default()
        }
    }

    let spectrum = Spectrum {
        id: "scan=1".to_string(),
        binary_data_array_list: Some(BinaryDataArrayList {
            count: Some(2),
            binary_data_arrays: vec![
                make_bda("MS:1000514", "m/z array", mz_data.clone()),
                make_bda("MS:1000515", "intensity array", int_data.clone()),
            ],
        }),
        ..Default::default()
    };

    let mzml_in = MzML {
        run: Run {
            spectrum_list: Some(SpectrumList {
                spectra: vec![spectrum],
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut encoded = Vec::new();
    write_mzml_to_ion(
        &mzml_in,
        WriteOptions {
            compression_level: 3,
            force_f32: false,
            block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
            parallel: true,
            section_storage: SectionStorage::Memory,
            mz_window: 0.0,
        },
        &mut encoded,
    )
    .unwrap();

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    let mzml_out = decoder.to_mzml().unwrap();

    let spectra = mzml_out.run.spectrum_list.unwrap().spectra;
    let arrays = spectra[0]
        .binary_data_array_list
        .as_ref()
        .unwrap()
        .binary_data_arrays
        .as_slice();

    let mz_out = arrays
        .iter()
        .find(|a| {
            a.cv_params
                .iter()
                .any(|cv| cv.accession.as_deref() == Some("MS:1000514"))
        })
        .and_then(|a| a.binary.as_ref())
        .unwrap();
    let int_out = arrays
        .iter()
        .find(|a| {
            a.cv_params
                .iter()
                .any(|cv| cv.accession.as_deref() == Some("MS:1000515"))
        })
        .and_then(|a| a.binary.as_ref())
        .unwrap();

    let NumericArray::F64(mz_vec) = mz_out else {
        panic!("expected F64 mz array");
    };
    let NumericArray::F64(int_vec) = int_out else {
        panic!("expected F64 intensity array");
    };

    assert_eq!(mz_vec.len(), n);
    assert_eq!(int_vec.len(), n);
    assert_eq!(mz_vec, &mz_data);
    assert_eq!(int_vec, &int_data);
}

#[test]
fn oversized_array_roundtrips_through_encode_decode() {
    use crate::ion::encoder::{
        encode::{WriteOptions, TARGET_BLOCK_UNCOMPRESSED_BYTES},
        ion_writer::write_mzml_to_ion,
    };
    use crate::mzml::structs::{
        NumericArray, BinaryDataArray, BinaryDataArrayList, CvParam, MzML, Run, Spectrum,
        SpectrumList,
    };

    let n = (TARGET_BLOCK_UNCOMPRESSED_BYTES / 8) * 2;
    let mz_data: Vec<f64> = (0..n).map(|i| i as f64 * 0.001).collect();
    let int_data: Vec<f64> = (0..n).map(|i| i as f64 * 10.0).collect();

    fn make_bda(accession: &str, name: &str, data: Vec<f64>) -> BinaryDataArray {
        BinaryDataArray {
            cv_params: vec![CvParam {
                cv_ref: Some("MS".to_string()),
                accession: Some(accession.to_string()),
                name: name.to_string(),
                value: None,
                unit_cv_ref: None,
                unit_name: None,
                unit_accession: None,
            }],
            binary: Some(NumericArray::F64(data)),
            ..Default::default()
        }
    }

    let spectrum = Spectrum {
        id: "scan=1".to_string(),
        binary_data_array_list: Some(BinaryDataArrayList {
            count: Some(2),
            binary_data_arrays: vec![
                make_bda("MS:1000514", "m/z array", mz_data.clone()),
                make_bda("MS:1000515", "intensity array", int_data.clone()),
            ],
        }),
        ..Default::default()
    };

    let mzml_in = MzML {
        run: Run {
            spectrum_list: Some(SpectrumList {
                spectra: vec![spectrum],
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut encoded = Vec::new();
    write_mzml_to_ion(
        &mzml_in,
        WriteOptions {
            compression_level: 0,
            force_f32: false,
            block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
            parallel: false,
            section_storage: SectionStorage::Memory,
            mz_window: 0.0,
        },
        &mut encoded,
    )
    .unwrap();

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    let mzml_out = decoder.to_mzml().unwrap();

    let spectra = mzml_out.run.spectrum_list.unwrap().spectra;
    let arrays = spectra[0]
        .binary_data_array_list
        .as_ref()
        .unwrap()
        .binary_data_arrays
        .as_slice();

    let mz_out = arrays
        .iter()
        .find(|a| {
            a.cv_params
                .iter()
                .any(|cv| cv.accession.as_deref() == Some("MS:1000514"))
        })
        .and_then(|a| a.binary.as_ref())
        .unwrap();
    let int_out = arrays
        .iter()
        .find(|a| {
            a.cv_params
                .iter()
                .any(|cv| cv.accession.as_deref() == Some("MS:1000515"))
        })
        .and_then(|a| a.binary.as_ref())
        .unwrap();

    let NumericArray::F64(mz_vec) = mz_out else {
        panic!("expected F64 mz array");
    };
    let NumericArray::F64(int_vec) = int_out else {
        panic!("expected F64 intensity array");
    };

    assert_eq!(mz_vec.len(), n);
    assert_eq!(int_vec.len(), n);
    assert_eq!(mz_vec, &mz_data);
    assert_eq!(int_vec, &int_data);
}

fn make_split_bda(accession: &str, name: &str, data: Vec<f64>) -> BinaryDataArray {
    BinaryDataArray {
        cv_params: vec![CvParam {
            cv_ref: Some("MS".to_string()),
            accession: Some(accession.to_string()),
            name: name.to_string(),
            value: None,
            unit_cv_ref: None,
            unit_name: None,
            unit_accession: None,
        }],
        binary: Some(NumericArray::F64(data)),
        ..Default::default()
    }
}

fn encode_one_spectrum_windowed(
    mz: Vec<f64>,
    int: Vec<f64>,
    mz_window: f64,
) -> Vec<u8> {
    encode_one_spectrum_windowed_mode(mz, int, mz_window, SectionStorage::Memory)
}

fn encode_one_spectrum_windowed_mode(
    mz: Vec<f64>,
    int: Vec<f64>,
    mz_window: f64,
    mode: SectionStorage,
) -> Vec<u8> {
    use crate::ion::encoder::{
        encode::{TARGET_BLOCK_UNCOMPRESSED_BYTES, WriteOptions},
        ion_writer::write_mzml_to_ion,
    };
    use crate::mzml::structs::{BinaryDataArrayList, MzML, Run, Spectrum, SpectrumList};

    let spectrum = Spectrum {
        id: "split_ms1".to_string(),
        binary_data_array_list: Some(BinaryDataArrayList {
            count: Some(2),
            binary_data_arrays: vec![
                make_split_bda("MS:1000514", "m/z array", mz),
                make_split_bda("MS:1000515", "intensity array", int),
            ],
        }),
        ..Default::default()
    };

    let mzml_in = MzML {
        run: Run {
            spectrum_list: Some(SpectrumList {
                spectra: vec![spectrum],
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut encoded = Vec::new();
    write_mzml_to_ion(
        &mzml_in,
        WriteOptions {
            compression_level: 3,
            force_f32: false,
            block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
            parallel: true,
            section_storage: mode,
            mz_window,
        },
        &mut encoded,
    )
    .unwrap();
    encoded
}

#[test]
fn to_mzml_keeps_all_spectra_across_metadata_group_boundaries() {
    use crate::ion::encoder::{
        encode::{WriteOptions, TARGET_BLOCK_UNCOMPRESSED_BYTES},
        ion_writer::write_mzml_to_ion,
    };
    use crate::mzml::structs::{BinaryDataArrayList, MzML, Run, Spectrum, SpectrumList};

    let spectrum_count = 8192 + 5;
    let spectra: Vec<Spectrum> = (0..spectrum_count)
        .map(|i| Spectrum {
            id: format!("scan={i}"),
            binary_data_array_list: Some(BinaryDataArrayList {
                count: Some(2),
                binary_data_arrays: vec![
                    make_split_bda("MS:1000514", "m/z array", vec![100.0, 100.5]),
                    make_split_bda("MS:1000515", "intensity array", vec![10.0, 20.0]),
                ],
            }),
            ..Default::default()
        })
        .collect();

    let mzml_in = MzML {
        run: Run {
            spectrum_list: Some(SpectrumList {
                spectra,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut encoded = Vec::new();
    write_mzml_to_ion(
        &mzml_in,
        WriteOptions {
            block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
            ..WriteOptions::quick(3, false)
        },
        &mut encoded,
    )
    .unwrap();

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    assert_eq!(decoder.header.spectrum_count, spectrum_count as u64);

    let out = decoder.to_mzml().unwrap();
    let spectra_out = out.run.spectrum_list.unwrap().spectra;
    assert_eq!(
        spectra_out.len(),
        spectrum_count,
        "to_mzml must not drop spectra past the first metadata group"
    );
    assert_eq!(spectra_out[0].id, "scan=0");
    assert_eq!(
        spectra_out[spectrum_count - 1].id,
        format!("scan={}", spectrum_count - 1)
    );
}

#[test]
fn to_mzml_keeps_all_chromatograms_across_metadata_group_boundaries() {
    use crate::ion::encoder::{
        encode::{WriteOptions, TARGET_BLOCK_UNCOMPRESSED_BYTES},
        ion_writer::write_mzml_to_ion,
    };
    use crate::mzml::structs::{
        BinaryDataArrayList, Chromatogram, ChromatogramList, MzML, Run,
    };

    let chromatogram_count = 8192 + 5;
    let chromatograms: Vec<Chromatogram> = (0..chromatogram_count)
        .map(|i| Chromatogram {
            id: format!("chrom={i}"),
            binary_data_array_list: Some(BinaryDataArrayList {
                count: Some(2),
                binary_data_arrays: vec![
                    make_split_bda("MS:1000595", "time array", vec![0.0, 1.0]),
                    make_split_bda("MS:1000515", "intensity array", vec![10.0, 20.0]),
                ],
            }),
            ..Default::default()
        })
        .collect();

    let mzml_in = MzML {
        run: Run {
            chromatogram_list: Some(ChromatogramList {
                count: Some(chromatogram_count),
                default_data_processing_ref: None,
                chromatograms,
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut encoded = Vec::new();
    write_mzml_to_ion(
        &mzml_in,
        WriteOptions {
            block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
            ..WriteOptions::quick(3, false)
        },
        &mut encoded,
    )
    .unwrap();

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    assert_eq!(decoder.header.chrom_count, chromatogram_count as u64);

    let out = decoder.to_mzml().unwrap();
    let chroms_out = out.run.chromatogram_list.unwrap().chromatograms;
    assert_eq!(
        chroms_out.len(),
        chromatogram_count,
        "to_mzml must not drop chromatograms past the first metadata group"
    );
    assert_eq!(chroms_out[0].id, "chrom=0");
    assert_eq!(
        chroms_out[chromatogram_count - 1].id,
        format!("chrom={}", chromatogram_count - 1)
    );
}

#[test]
fn split_mz_array_roundtrips_through_to_mzml() {
    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| (i % 1000) as f64).collect();

    let encoded =
        encode_one_spectrum_windowed(mz.clone(), int.clone(), 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    assert!(
        decoder.header.spec_block_count >= 4,
        "splitting must produce several window blocks, got {}",
        decoder.header.spec_block_count
    );

    let mzml_out = decoder.to_mzml().unwrap();
    let spectra = mzml_out.run.spectrum_list.unwrap().spectra;
    assert_eq!(spectra.len(), 1);

    let arrays = &spectra[0]
        .binary_data_array_list
        .as_ref()
        .unwrap()
        .binary_data_arrays;
    let mz_arrays: Vec<_> = arrays
        .iter()
        .filter(|a| {
            a.cv_params
                .iter()
                .any(|cv| cv.accession.as_deref() == Some("MS:1000514"))
        })
        .collect();
    assert_eq!(
        mz_arrays.len(),
        1,
        "split windows must reconstruct one logical m/z array"
    );

    let NumericArray::F64(mz_out) = mz_arrays[0].binary.as_ref().unwrap() else {
        panic!("expected F64 mz");
    };
    assert_eq!(mz_out, &mz);
}

#[test]
fn split_mz_array_roundtrips_through_get_spectrum() {
    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| (i % 777) as f64).collect();

    let encoded =
        encode_one_spectrum_windowed(mz.clone(), int.clone(), 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    let spectrum = decoder.spectrum(0).unwrap().unwrap();
    let arrays = &spectrum
        .binary_data_array_list
        .as_ref()
        .unwrap()
        .binary_data_arrays;

    let mz_arrays: Vec<_> = arrays
        .iter()
        .filter(|a| {
            a.cv_params
                .iter()
                .any(|cv| cv.accession.as_deref() == Some("MS:1000514"))
        })
        .collect();
    assert_eq!(mz_arrays.len(), 1);
    let NumericArray::F64(mz_out) = mz_arrays[0].binary.as_ref().unwrap() else {
        panic!("expected F64 mz");
    };
    assert_eq!(mz_out, &mz);

    let int_arrays: Vec<_> = arrays
        .iter()
        .filter(|a| {
            a.cv_params
                .iter()
                .any(|cv| cv.accession.as_deref() == Some("MS:1000515"))
        })
        .collect();
    assert_eq!(int_arrays.len(), 1);
    let NumericArray::F64(int_out) = int_arrays[0].binary.as_ref().unwrap() else {
        panic!("expected F64 intensity");
    };
    assert_eq!(int_out, &int);
}

#[test]
fn read_spectrum_logical_array_joins_split_segments() {
    let n = 40_000;
    let mz: Vec<f64> = (0..n).map(|i| 200.0 + i as f64 * 0.002).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();

    let encoded = encode_one_spectrum_windowed(mz.clone(), int.clone(), 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    let mz_out = decoder
        .read_spectrum_logical_array(0, crate::accessions::MZ_ARRAY)
        .unwrap();
    assert_eq!(mz_out, mz);
}

#[test]
fn centroided_small_arrays_are_encoded_correctly() {
    let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
    let int: Vec<f64> = (0..10).map(|i| i as f64).collect();

    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    IonReader::open(&encoded, ReadOptions::default()).unwrap();
}

#[test]
fn split_mz_array_roundtrips_with_disk_staged_bounds() {
    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| (i % 1000) as f64).collect();

    let encoded = encode_one_spectrum_windowed_mode(
        mz.clone(),
        int.clone(),
        10.0,
        SectionStorage::Disk,
    );

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();

    let mz_out = decoder
        .read_spectrum_logical_array(0, crate::accessions::MZ_ARRAY)
        .unwrap();
    assert_eq!(mz_out, mz);
}

fn brute_force_window(mz: &[f64], int: &[f64], low: f64, high: f64) -> (Vec<f64>, Vec<f64>) {
    let mut mz_out = Vec::new();
    let mut int_out = Vec::new();
    for (index, &value) in mz.iter().enumerate() {
        if value >= low && value <= high {
            mz_out.push(value);
            int_out.push(int[index]);
        }
    }
    (mz_out, int_out)
}

#[test]
fn window_fast_path_matches_brute_force() {
    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let encoded =
        encode_one_spectrum_windowed(mz.clone(), int.clone(), 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();

    let windows = [
        (120.0, 130.0),
        (100.0, 149.999),
        (100.0001, 100.0009),
        (130.5, 130.5),
        (200.0, 300.0),
        (0.0, 50.0),
    ];
    for (low, high) in windows {
        let got = decoder.read_window(0, Range { from: low, to: high }).unwrap();
        let (expected_mz, expected_int) = brute_force_window(&mz, &int, low, high);
        assert_eq!(got.x.to_f64(), expected_mz, "mz mismatch for window {low}..{high}");
        assert_eq!(
            got.y.to_f64(), expected_int,
            "intensity mismatch for window {low}..{high}"
        );
    }

    assert!(
        matches!(decoder.spec_window_bounds, WindowBoundsCache::Loaded(_)),
        "fast path should have loaded A3"
    );
}

#[test]
fn window_errors_when_bounds_missing() {
    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let encoded =
        encode_one_spectrum_windowed(mz, int, 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    decoder.spec_window_bounds = WindowBoundsCache::Missing;

    assert_eq!(
        decoder.read_window(0, Range { from: 120.0, to: 130.0 }),
        Err(IonError::MissingSpectrumBounds)
    );
}

#[test]
fn window_on_unsplit_array_uses_fallback_and_is_correct() {
    let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
    let int: Vec<f64> = (0..10).map(|i| (i * 7) as f64).collect();
    let encoded =
        encode_one_spectrum_windowed(mz.clone(), int.clone(), 0.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();

    let got = decoder.read_window(0, Range { from: 102.0, to: 105.0 }).unwrap();
    let (expected_mz, expected_int) = brute_force_window(&mz, &int, 102.0, 105.0);
    assert_eq!(got.x.to_f64(), expected_mz);
    assert_eq!(got.y.to_f64(), expected_int);
}

#[test]
fn window_out_of_range_index_errors() {
    let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
    let int: Vec<f64> = (0..10).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    assert!(decoder.read_window(5, Range { from: 100.0, to: 200.0 }).is_err());
}

#[test]
fn reader_reads_mz_range() {
    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let encoded =
        encode_one_spectrum_windowed(mz.clone(), int.clone(), 10.0);

    let bytes_arc: Arc<[u8]> = Arc::from(encoded.as_slice());
    let mut reader = IonReader::open_bytes(bytes_arc, ReadOptions::default()).unwrap();
    let got = reader.read_window(0, Range { from: 120.0, to: 130.0 }).unwrap();
    let (expected_mz, expected_int) = brute_force_window(&mz, &int, 120.0, 130.0);
    assert_eq!(got.x.to_f64(), expected_mz);
    assert_eq!(got.y.to_f64(), expected_int);
}

#[test]
fn a3_is_sparse_rows_for_mz_segments_none_for_intensity() {
    use crate::ion::axes::{Axis, axis_of};

    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    let _ = decoder.read_window(0, Range { from: 120.0, to: 130.0 }).unwrap();
    let WindowBoundsCache::Loaded(index) = &decoder.spec_window_bounds else {
        panic!("A3 should be loaded");
    };

    let array_refs =
        read_array_refs_from_buffers(&decoder.spec_entries_buf, &decoder.spec_array_refs, 0)
            .unwrap();
    let groups = group_arrays(array_refs.as_slice()).unwrap();

    let mut position = 0u64;
    let mut mz_range = None;
    let mut intensity_range = None;
    for group in &groups {
        let count = group.refs.len() as u64;
        if matches!(axis_of(group.array_type), Some(Axis::Mz)) {
            mz_range = Some((position, count));
        } else if group.array_type == ACC_INT {
            intensity_range = Some((position, count));
        }
        position += count;
    }

    let (mz_base, mz_count) = mz_range.unwrap();
    let (intensity_base, intensity_count) = intensity_range.unwrap();
    assert!(mz_count >= 2, "expected the m/z array to be split");
    assert_eq!(mz_count, intensity_count);

    for i in 0..mz_count {
        assert!(
            index.get(mz_base + i).is_some(),
            "missing A3 row for m.z window {i}"
        );
    }
    for i in 0..intensity_count {
        assert!(
            index.get(intensity_base + i).is_none(),
            "intensity window {i} must not have an A3 row"
        );
    }
}

#[test]
fn generic_window_matches_read_mz_range() {
    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();

    let generic = decoder
        .read_spectrum_window(0, crate::accessions::MZ_ARRAY, ACC_INT, 120.0, 130.0)
        .unwrap();

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    let mz_range = decoder.read_window(0, Range { from: 120.0, to: 130.0 }).unwrap();

    assert_eq!(generic.x, mz_range.x);
    assert_eq!(generic.y, mz_range.y);
}

#[test]
fn generic_window_missing_accession_returns_empty() {
    let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
    let int: Vec<f64> = (0..10).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();

    let got = decoder
        .read_spectrum_window(0, 99_999_999, ACC_INT, 1.0, 2.0)
        .unwrap();

    assert!(got.x.is_empty());
    assert!(got.y.is_empty());
}

fn spec_directory_range(header: &Header) -> (usize, usize) {
    let entry_size =
        crate::ion::encoder::utilities::block_writer::BLOCK_DIRECTORY_ENTRY_SIZE as u64;
    let directory_size = header.spec_block_count * entry_size;
    let end = header.off_spec_container + header.len_spec_container;
    let start = end - directory_size;
    (start as usize, end as usize)
}

#[test]
fn directory_crc_roundtrips() {
    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let header = parse_header(&encoded[..1024]).unwrap();
    let (start, end) = spec_directory_range(&header);
    let computed = crc32fast::hash(&encoded[start..end]);

    assert_eq!(computed, header.spec_directory_crc32);
    assert!(IonReader::open(&encoded, ReadOptions::default()).is_ok());
}

#[test]
fn flipped_directory_offset_is_caught_before_any_read() {
    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let mut encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let header = parse_header(&encoded[..1024]).unwrap();
    let (start, _end) = spec_directory_range(&header);
    encoded[start] ^= 0xFF;

    let result = IonReader::open(&encoded, ReadOptions::default());
    assert!(result.is_err());
    let message = format!("{}", result.err().unwrap());
    assert!(message.contains("directory checksum mismatch"));
}

#[test]
fn verify_off_skips_directory_check() {
    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let mut encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let header = parse_header(&encoded[..1024]).unwrap();
    let (start, _end) = spec_directory_range(&header);
    encoded[start] ^= 0xFF;

    let config = ReadOptions {
        verify_checksums: false,
        ..ReadOptions::default()
    };
    assert!(IonReader::open(&encoded, config).is_ok());
}

#[test]
fn empty_container_directory_crc_is_consistent() {
    let mz: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
    let int: Vec<f64> = (0..10).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let header = parse_header(&encoded[..1024]).unwrap();
    assert_eq!(header.chrom_block_count, 0);
    assert_eq!(header.chrom_directory_crc32, crc32fast::hash(&[]));
    assert!(IonReader::open(&encoded, ReadOptions::default()).is_ok());
}

#[test]
fn a3_b3_are_core_sections() {
    let mz: Vec<f64> = (0..1000).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..1000).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let header = parse_header(&encoded[..1024]).unwrap();

    assert!(
        header.len_spec_window_bounds > 0,
        "A3 should be written as core section"
    );
}

#[test]
fn candidate_items_filters_by_axis_accession() {
    use crate::accessions::INTENSITY_ARRAY;

    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();

    let mz_candidates = decoder
        .candidate_items(Target::Spec, crate::accessions::MZ_ARRAY, 120.0, 130.0)
        .unwrap();
    let int_candidates = decoder
        .candidate_items(Target::Spec, INTENSITY_ARRAY, 1000.0, 2000.0)
        .unwrap();

    assert!(
        !mz_candidates.is_empty(),
        "m/z candidates should be found with bounds"
    );
    assert!(
        int_candidates.is_empty(),
        "intensity is not an axis, so no candidates"
    );
}

#[test]
fn candidate_items_falls_back_when_bounds_missing() {
    let n = 1000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();

    decoder.spec_window_bounds = WindowBoundsCache::Missing;

    let candidates = decoder
        .candidate_items(Target::Spec, crate::accessions::MZ_ARRAY, 120.0, 130.0)
        .unwrap();

    assert!(
        !candidates.is_empty(),
        "should return candidates when bounds unavailable (fallback)"
    );
}

#[test]
fn core_a3_crc_failure_falls_back() {
    let n = 5000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let mut encoded =
        encode_one_spectrum_windowed(mz.clone(), int.clone(), 10.0);

    let a3_offset = {
        let header = parse_header(&encoded[..1024]).unwrap();
        header.off_spec_window_bounds as usize
    };

    if a3_offset > 0 {
        encoded[a3_offset] ^= 0xFF;

        let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
        let candidates = decoder
            .candidate_items(Target::Spec, crate::accessions::MZ_ARRAY, 120.0, 130.0)
            .unwrap();

        assert!(
            !candidates.is_empty(),
            "should return all candidates when A3 CRC fails"
        );
        assert!(
            matches!(decoder.spec_window_bounds, WindowBoundsCache::BadChecksum),
            "A3 should be marked bad checksum after CRC failure"
        );
    }
}

#[test]
fn b3_header_fields_are_populated() {
    let n = 5000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let header = parse_header(&encoded[..1024]).unwrap();

    assert_eq!(
        header.off_chrom_window_bounds, 0,
        "B3 offset should be 0 (no chromatograms)"
    );
    assert_eq!(
        header.len_chrom_window_bounds, 0,
        "B3 length should be 0 (no chromatograms)"
    );
    assert_eq!(
        header.plain_len_chrom_window_bounds, 0,
        "B3 plain_len should be 0 (no chromatograms)"
    );
}

#[test]
fn candidate_items_for_chrom_axis_without_bounds() {
    let n = 1000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz, int, 10.0);

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();

    let candidates = decoder
        .candidate_items(Target::Chrom, crate::accessions::TIME_ARRAY, 0.0, 1000.0)
        .unwrap();

    assert!(
        candidates.is_empty(),
        "chromatogram query should return empty when file has no chromatograms"
    );
}

fn split_file_with_a3() -> (Vec<f64>, Vec<f64>, Vec<u8>) {
    let n = 50_000;
    let mz: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 0.001).collect();
    let int: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let encoded = encode_one_spectrum_windowed(mz.clone(), int.clone(), 10.0);
    (mz, int, encoded)
}

fn kept_block_ids(
    decoder: &IonReader,
    scan_index: usize,
    low: f64,
    high: f64,
) -> (Vec<u32>, Vec<u32>) {
    let array_refs = read_array_refs_from_buffers(
        &decoder.spec_entries_buf,
        &decoder.spec_array_refs,
        scan_index,
    )
    .unwrap();
    let ref_start = array_ref_start_for_item(&decoder.spec_entries_buf, scan_index).unwrap();
    let groups = group_arrays(array_refs.as_slice()).unwrap();

    let mut mz_group = None;
    let mut intensity_group = None;
    let mut position = 0u64;
    for group in &groups {
        if group.array_type == crate::accessions::MZ_ARRAY && mz_group.is_none() {
            mz_group = Some((position, group));
        } else if group.array_type == ACC_INT && intensity_group.is_none() {
            intensity_group = Some((position, group));
        }
        position += group.refs.len() as u64;
    }
    let (mz_position, mz_group) = mz_group.unwrap();
    let (_, intensity_group) = intensity_group.unwrap();

    let WindowBoundsCache::Loaded(index) = &decoder.spec_window_bounds else {
        panic!("A3 must be loaded before computing kept blocks");
    };

    let mut mz_blocks = Vec::new();
    let mut intensity_blocks = Vec::new();
    for window in 0..mz_group.refs.len() {
        let (window_low, window_high) =
            index.get(ref_start + mz_position + window as u64).unwrap();
        if window_low <= high && window_high >= low {
            mz_blocks.push(mz_group.refs[window].block_id);
            intensity_blocks.push(intensity_group.refs[window].block_id);
        }
    }
    mz_blocks.sort_unstable();
    mz_blocks.dedup();
    intensity_blocks.sort_unstable();
    intensity_blocks.dedup();
    (mz_blocks, intensity_blocks)
}

fn block_ranges_for(decoder: &IonReader, block_ids: &[u32]) -> Vec<ByteRange> {
    let mut ranges: Vec<ByteRange> = block_ids
        .iter()
        .map(|&id| decoder.spec_container.block_byte_range(id).unwrap())
        .collect();
    ranges.sort_unstable_by_key(|range| (range.offset, range.length));
    ranges.dedup();
    ranges
}

#[test]
fn require_bounds_passes_on_file_with_a3() {
    let (_, _, encoded) = split_file_with_a3();
    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    assert!(decoder.require_bounds().is_ok());
}

#[test]
fn require_bounds_errors_when_a3_missing() {
    let (_, _, encoded) = split_file_with_a3();
    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    decoder.spec_window_bounds = WindowBoundsCache::Missing;
    assert_eq!(
        decoder.require_bounds(),
        Err(IonError::MissingSpectrumBounds)
    );
}

#[test]
fn require_bounds_errors_on_bad_checksum() {
    let (_, _, mut encoded) = split_file_with_a3();
    let a3_offset = parse_header(&encoded[..1024]).unwrap().off_spec_window_bounds as usize;
    assert!(a3_offset > 0);
    encoded[a3_offset] ^= 0xFF;

    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    assert_eq!(
        decoder.require_bounds(),
        Err(IonError::BadSpectrumBoundsChecksum)
    );
}

#[test]
fn require_bounds_errors_on_malformed_rows() {
    let (_, _, encoded) = split_file_with_a3();
    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    decoder.spec_window_bounds = WindowBoundsCache::Malformed("bad rows".to_string());
    assert_eq!(
        decoder.require_bounds(),
        Err(IonError::MalformedSpectrumBounds("bad rows".to_string()))
    );
}

#[test]
fn read_mz_range_matches_brute_force() {
    let (mz, int, encoded) = split_file_with_a3();
    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();

    for (low, high) in [(120.0, 130.0), (100.0, 149.999), (130.5, 130.5)] {
        let got = decoder.read_window(0, Range { from: low, to: high }).unwrap();
        let (expected_mz, expected_int) = brute_force_window(&mz, &int, low, high);
        assert_eq!(got.x.to_f64(), expected_mz, "mz mismatch for window {low}..{high}");
        assert_eq!(
            got.y.to_f64(), expected_int,
            "intensity mismatch for window {low}..{high}"
        );
    }
}

#[test]
fn read_mz_range_errors_when_a3_missing() {
    let (_, _, encoded) = split_file_with_a3();
    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    decoder.spec_window_bounds = WindowBoundsCache::Missing;
    assert_eq!(
        decoder.read_window(0, Range { from: 120.0, to: 130.0 }),
        Err(IonError::MissingSpectrumBounds)
    );
}

#[test]
fn read_mz_range_errors_when_a3_row_missing() {
    let (_, _, encoded) = split_file_with_a3();
    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    decoder.require_bounds().unwrap();

    let array_refs =
        read_array_refs_from_buffers(&decoder.spec_entries_buf, &decoder.spec_array_refs, 0)
            .unwrap();
    let groups = group_arrays(array_refs.as_slice()).unwrap();
    let mz_count = groups
        .iter()
        .find(|group| group.array_type == crate::accessions::MZ_ARRAY)
        .unwrap()
        .refs
        .len();
    assert!(mz_count >= 2, "need a split m/z array for this test");

    let total_refs = (decoder.spec_array_refs.len() / ARRAY_REF_BYTES) as u64;
    let mut rows = Vec::new();
    for row_index in 0..(mz_count as u64 - 1) {
        rows.extend_from_slice(&row_index.to_le_bytes());
        rows.extend_from_slice(&0.0f64.to_le_bytes());
        rows.extend_from_slice(&1.0e9f64.to_le_bytes());
    }
    let index = WindowBoundsIndex::from_bytes(&rows, total_refs).unwrap();
    decoder.spec_window_bounds = WindowBoundsCache::Loaded(index);

    let result = decoder.read_window(0, Range { from: 100.0, to: 200.0 });
    assert!(matches!(result, Err(IonError::MalformedSpectrumBounds(_))));
}

#[test]
fn read_mz_range_errors_on_low_above_high() {
    let (_, _, encoded) = split_file_with_a3();
    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    assert!(decoder.read_window(0, Range { from: 130.0, to: 120.0 }).is_err());
}

#[test]
fn read_mz_range_errors_on_non_finite_bounds() {
    let (_, _, encoded) = split_file_with_a3();
    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();
    assert!(
        decoder
            .read_window(0, Range { from: f64::NAN, to: 130.0 })
            .is_err()
    );
    assert!(
        decoder
            .read_window(0, Range { from: 120.0, to: f64::INFINITY })
            .is_err()
    );
}

#[test]
fn plan_mz_range_returns_mz_and_intensity_blocks() {
    let (_, _, encoded) = split_file_with_a3();
    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();

    let plan = decoder
        .byte_ranges(0, Range { from: 120.0, to: 130.0 })
        .unwrap();

    let (mz_blocks, intensity_blocks) = kept_block_ids(&decoder, 0, 120.0, 130.0);
    assert!(!mz_blocks.is_empty(), "expected kept m/z blocks");
    assert!(!intensity_blocks.is_empty(), "expected kept intensity blocks");

    for range in block_ranges_for(&decoder, &intensity_blocks) {
        assert!(
            plan.contains(&range),
            "plan must include intensity block range {range:?}"
        );
    }
    for range in block_ranges_for(&decoder, &mz_blocks) {
        assert!(
            plan.contains(&range),
            "plan must include m/z block range {range:?}"
        );
    }
}

#[test]
fn plan_and_read_mz_range_use_same_segments() {
    let (_, _, encoded) = split_file_with_a3();
    let mut decoder = IonReader::open(&encoded, ReadOptions::default()).unwrap();

    let plan = decoder
        .byte_ranges(0, Range { from: 120.0, to: 130.0 })
        .unwrap();

    let (mz_blocks, intensity_blocks) = kept_block_ids(&decoder, 0, 120.0, 130.0);
    let mut all_blocks = mz_blocks;
    all_blocks.extend(intensity_blocks);
    all_blocks.sort_unstable();
    all_blocks.dedup();

    let expected = block_ranges_for(&decoder, &all_blocks);
    assert_eq!(plan, expected);
}

#[test]
fn plan_open_ranges_includes_spec_a3() {
    let (_, _, encoded) = split_file_with_a3();
    let header = parse_header(&encoded[..1024]).unwrap();
    assert!(header.len_spec_window_bounds > 0);

    let ranges = open_ranges(&encoded[..1024]).unwrap();
    assert!(ranges.contains(&ByteRange {
        offset: header.off_spec_window_bounds,
        length: header.len_spec_window_bounds,
    }));
}

#[test]
fn plan_open_ranges_includes_container_directories() {
    let (_, _, encoded) = split_file_with_a3();
    let header = parse_header(&encoded[..1024]).unwrap();
    let (start, end) = spec_directory_range(&header);

    let ranges = open_ranges(&encoded[..1024]).unwrap();
    assert!(ranges.contains(&ByteRange {
        offset: start as u64,
        length: (end - start) as u64,
    }));
}

#[test]
fn reader_plans_and_reads_mz_range() {
    let (mz, int, encoded) = split_file_with_a3();
    let bytes: Arc<[u8]> = Arc::from(encoded.as_slice());
    let mut reader = IonReader::open_bytes(bytes, ReadOptions::default()).unwrap();

    reader.require_bounds().unwrap();

    let plan = reader
        .byte_ranges(0, Range { from: 120.0, to: 130.0 })
        .unwrap();
    assert!(!plan.is_empty());

    let got = reader.read_window(0, Range { from: 120.0, to: 130.0 }).unwrap();
    let (expected_mz, expected_int) = brute_force_window(&mz, &int, 120.0, 130.0);
    assert_eq!(got.x.to_f64(), expected_mz);
    assert_eq!(got.y.to_f64(), expected_int);
}
