//! Exercises the binary→mzML XML serialization path with synthetic data, verifying that the output is valid XML that can be re-parsed into equivalent MzML structs.
mod common;

use common::binary_ext::BinaryDataExt;
use common::helpers::{
    build_mzml, default_cv_list_like_writer, make_chromatogram_f64, make_spectrum_f64,
    minimal_file_description, synthetic_binary_data_array,
};
use ionic::mzml::{
    bin_to_mzml::{bin_to_mzml, convert_bin_to_mzml_bytes},
    parse_mzml::parse_mzml,
    structs::*,
};

#[test]
fn convert_bin_to_mzml_bytes_produces_valid_xml() {
    let mz = vec![100.0, 200.0, 300.0];
    let intensity = vec![1000.0, 2000.0, 3000.0];
    let mzml = build_mzml(
        vec![make_spectrum_f64("scan=1", mz.clone(), intensity.clone())],
        vec![],
    );

    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("should succeed");
    let xml_str = String::from_utf8(bytes.clone()).expect("should be valid UTF-8");
    assert!(xml_str.contains("<?xml"), "should have XML declaration");
    assert!(xml_str.contains("<mzML"), "should have <mzML> tag");
    assert!(
        xml_str.contains("</mzML>"),
        "should have closing </mzML> tag"
    );
    let reparsed = parse_mzml(&bytes).expect("output should be re-parseable");
    let spectra = reparsed
        .run
        .spectrum_list
        .as_ref()
        .expect("should have spectrum list");
    assert_eq!(spectra.spectra.len(), 1);
}

#[test]
fn bin_to_mzml_string_version() {
    let mzml = build_mzml(
        vec![make_spectrum_f64(
            "scan=1",
            vec![1.0, 2.0],
            vec![10.0, 20.0],
        )],
        vec![],
    );

    let xml_str = bin_to_mzml(&mzml).expect("should succeed");
    assert!(xml_str.contains("<mzML"));
    assert!(xml_str.contains("scan=1"));
}

#[test]
fn roundtrip_preserves_mz_values() {
    let mz = vec![100.123, 200.456, 300.789, 400.012, 500.345];
    let intensity = vec![1e3, 2e3, 3e3, 4e3, 5e3];
    let mzml = build_mzml(
        vec![make_spectrum_f64("scan=1", mz.clone(), intensity.clone())],
        vec![],
    );

    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("encode should succeed");
    let reparsed = parse_mzml(&bytes).expect("re-parse should succeed");
    let spectra = reparsed.run.spectrum_list.as_ref().unwrap();
    let arrays = spectra.spectra[0].binary_data_array_list.as_ref().unwrap();
    let mz_bda = arrays
        .binary_data_arrays
        .iter()
        .find(|a| {
            a.cv_params
                .iter()
                .any(|p| p.accession.as_deref() == Some("MS:1000514"))
        })
        .expect("should have m/z array");

    let int_bda = arrays
        .binary_data_arrays
        .iter()
        .find(|a| {
            a.cv_params
                .iter()
                .any(|p| p.accession.as_deref() == Some("MS:1000515"))
        })
        .expect("should have intensity array");

    let got_mz = mz_bda.binary.as_ref().unwrap().to_f64_vec();
    let got_int = int_bda.binary.as_ref().unwrap().to_f64_vec();
    assert_eq!(got_mz, mz, "m/z values should be exactly preserved");
    assert_eq!(
        got_int, intensity,
        "intensity values should be exactly preserved"
    );
}

#[test]
fn roundtrip_with_chromatogram() {
    let time = vec![0.0, 1.0, 2.0, 3.0];
    let intensity = vec![100.0, 200.0, 150.0, 50.0];
    let mzml = build_mzml(
        vec![],
        vec![make_chromatogram_f64(
            "TIC",
            time.clone(),
            intensity.clone(),
        )],
    );

    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("encode should succeed");
    let reparsed = parse_mzml(&bytes).expect("re-parse should succeed");

    let chroms = reparsed.run.chromatogram_list.as_ref().unwrap();
    assert_eq!(chroms.chromatograms.len(), 1);
    assert_eq!(chroms.chromatograms[0].id, "TIC");
}

#[test]
fn roundtrip_multiple_spectra() {
    let spectra = vec![
        make_spectrum_f64("scan=1", vec![100.0, 200.0], vec![10.0, 20.0]),
        make_spectrum_f64("scan=2", vec![150.0, 250.0], vec![15.0, 25.0]),
        make_spectrum_f64("scan=3", vec![300.0], vec![30.0]),
    ];
    let spectra: Vec<Spectrum> = spectra
        .into_iter()
        .enumerate()
        .map(|(i, mut s)| {
            s.index = Some(i as u32);
            s
        })
        .collect();

    let mzml = build_mzml(spectra, vec![]);

    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("encode should succeed");
    let reparsed = parse_mzml(&bytes).expect("re-parse should succeed");

    let sl = reparsed.run.spectrum_list.as_ref().unwrap();
    assert_eq!(sl.spectra.len(), 3);
    assert_eq!(sl.spectra[0].id, "scan=1");
    assert_eq!(sl.spectra[1].id, "scan=2");
    assert_eq!(sl.spectra[2].id, "scan=3");
}

#[test]
fn missing_file_description_returns_error() {
    let mzml = MzML {
        file_description: None,
        run: Run {
            id: "no-fd".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };

    let result = convert_bin_to_mzml_bytes(&mzml);
    assert!(
        result.is_err(),
        "missing file_description should return error"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_lowercase().contains("filedescription")
            || err.to_lowercase().contains("file_description")
            || err.to_lowercase().contains("file description"),
        "error should mention file description, got: {err}"
    );
}

#[test]
fn empty_spectrum_list_produces_valid_xml() {
    let mzml = build_mzml(vec![], vec![]);

    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("should succeed");
    let xml_str = String::from_utf8(bytes.clone()).expect("valid UTF-8");
    assert!(xml_str.contains("<mzML"));
    let _ = parse_mzml(&bytes).expect("re-parse should succeed");
}

#[test]
fn f32_arrays_via_mzml_xml_roundtrip() {
    let mz = vec![100.0_f32, 200.0, 300.0];
    let intensity = vec![1000.0_f32, 2000.0, 3000.0];
    let len = mz.len();

    let mzml = MzML {
        cv_list: Some(default_cv_list_like_writer()),
        file_description: Some(minimal_file_description()),
        run: Run {
            id: "f32-test".to_string(),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: Some(len),
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(2),
                        binary_data_arrays: vec![
                            synthetic_binary_data_array(
                                "MS:1000514",
                                NumericType::Float32,
                                BinaryData::F32(mz.clone()),
                                Some(len),
                            ),
                            synthetic_binary_data_array(
                                "MS:1000515",
                                NumericType::Float32,
                                BinaryData::F32(intensity.clone()),
                                Some(len),
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

    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("encode should succeed");
    let reparsed = parse_mzml(&bytes).expect("re-parse should succeed");

    let sl = reparsed.run.spectrum_list.as_ref().unwrap();
    assert_eq!(sl.spectra.len(), 1);

    // Values should be recoverable (possibly promoted to f64)
    let arrays = sl.spectra[0].binary_data_array_list.as_ref().unwrap();
    for bda in &arrays.binary_data_arrays {
        let vals = bda.binary.as_ref().unwrap().to_f64_vec();
        assert_eq!(vals.len(), len);
    }
}
