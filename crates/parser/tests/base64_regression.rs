mod common;

use common::binary_ext::BinaryDataExt;
use common::helpers::{parse_single_array_xml, single_array_xml};
use ionic::mzml::{parse_mzml::parse_mzml, structs::*};

#[test]
fn known_f64_values_via_xml_roundtrip() {
    let values = vec![1.0_f64, 2.0, 3.0, 100.5, -42.0];
    let binary = NumericArray::F64(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Float64, &binary, None);
    let result = parse_single_array_xml(&xml).expect("should have binary data");
    let got = result.to_f64_vec();
    assert_eq!(got, values);
}

#[test]
fn known_f32_values_via_xml_roundtrip() {
    let values = vec![1.0_f32, 2.0, 3.0, 100.5, -42.0];
    let binary = NumericArray::F32(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Float32, &binary, None);
    let result = parse_single_array_xml(&xml).expect("should have binary data");
    match result {
        NumericArray::F32(got) => assert_eq!(got, values),
        NumericArray::F64(got) => {
            let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
            assert_eq!(got, expected);
        }
        other => panic!("unexpected variant: {}", other.variant_name()),
    }
}

#[test]
fn known_i32_values_via_xml_roundtrip() {
    let values = vec![0_i32, 1, -1, i32::MAX, i32::MIN];
    let binary = NumericArray::I32(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Int32, &binary, None);
    let result = parse_single_array_xml(&xml).expect("should have binary data");
    match result {
        NumericArray::I32(got) => assert_eq!(got, values),
        other => panic!("unexpected variant: {}", other.variant_name()),
    }
}

#[test]
fn known_i64_values_via_xml_roundtrip() {
    let values = vec![0_i64, 1, -1, i64::MAX, i64::MIN];
    let binary = NumericArray::I64(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Int64, &binary, None);
    let result = parse_single_array_xml(&xml).expect("should have binary data");
    match result {
        NumericArray::I64(got) => assert_eq!(got, values),
        other => panic!("unexpected variant: {}", other.variant_name()),
    }
}

#[test]
fn single_element_f64_roundtrip() {
    let values = vec![std::f64::consts::PI];
    let binary = NumericArray::F64(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Float64, &binary, None);
    let result = parse_single_array_xml(&xml).expect("should have binary data");
    let got = result.to_f64_vec();
    assert_eq!(got, values);
}

#[test]
fn empty_base64_produces_empty_array() {
    let values: Vec<f64> = vec![];
    let binary = NumericArray::F64(values);
    let xml = single_array_xml("MS:1000514", NumericType::Float64, &binary, Some(0));
    let result = parse_single_array_xml(&xml);
    match result {
        None => {}
        Some(bin) => assert_eq!(bin.len(), 0, "empty base64 should give empty array"),
    }
}

#[test]
fn large_array_base64_roundtrip() {
    let values: Vec<f64> = (0..1000).map(|i| i as f64 * 0.001).collect();
    let binary = NumericArray::F64(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Float64, &binary, None);
    let result = parse_single_array_xml(&xml).expect("should have binary data");
    let got = result.to_f64_vec();
    assert_eq!(got, values);
}

#[test]
fn base64_with_embedded_whitespace() {
    use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
    let bytes: Vec<u8> = [1.0_f64, 2.0_f64]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let encoded = BASE64_STANDARD.encode(&bytes);
    let with_whitespace = format!("{}\n{}", &encoded[..12], &encoded[12..]);

    let xml = format!(
        concat!(
            "<mzML>",
            "<fileDescription><fileContent/><sourceFileList count=\"0\"/></fileDescription>",
            "<run id=\"ws-b64\"><spectrumList count=\"1\">",
            "<spectrum index=\"0\" id=\"scan=1\" defaultArrayLength=\"2\">",
            "<binaryDataArrayList count=\"1\">",
            "<binaryDataArray encodedLength=\"{len}\">",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000514\" name=\"m/z array\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000523\" name=\"64-bit float\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000576\" name=\"no compression\"/>",
            "<binary>{base64}</binary>",
            "</binaryDataArray></binaryDataArrayList></spectrum></spectrumList>",
            "</run></mzML>"
        ),
        len = with_whitespace.len(),
        base64 = with_whitespace,
    );

    let mzml = parse_mzml(xml.as_bytes()).expect("should parse");
    let spectra = &mzml.run.spectrum_list.as_ref().unwrap().spectra;
    assert_eq!(spectra.len(), 1);
    let bda = &spectra[0]
        .binary_data_array_list
        .as_ref()
        .unwrap()
        .binary_data_arrays[0];
    let vals = bda.binary.as_ref().unwrap().to_f64_vec();
    assert_eq!(vals, vec![1.0, 2.0]);
}

#[test]
fn all_zeros_base64_roundtrip() {
    let values = vec![0.0_f64; 10];
    let binary = NumericArray::F64(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Float64, &binary, None);
    let result = parse_single_array_xml(&xml).expect("should have binary data");
    let got = result.to_f64_vec();
    assert_eq!(got, values);
}
