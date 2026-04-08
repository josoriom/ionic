//! T2-3: Base64 regression strings.
//!
//! Tests the mzML XML parser's base64 decoding pathway with known payloads:
//! empty base64, padded/unpadded, whitespace inside base64 content, and
//! known numeric values that map to specific base64 strings.

mod common;

use common::binary_ext::BinaryDataExt;
use common::builders::single_array_xml;
use ionic::mzml::{parse_mzml::parse_mzml, structs::*};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a single-array XML string and extract the binary data from the first
/// spectrum's first array.
fn parse_single_array(xml: &str) -> Option<BinaryData> {
    let mzml = parse_mzml(xml.as_bytes()).expect("parse should succeed");
    let spectra = mzml.run.spectrum_list.as_ref()?.spectra.first()?;
    let bda = spectra
        .binary_data_array_list
        .as_ref()?
        .binary_data_arrays
        .first()?;
    bda.binary.clone()
}

// ---------------------------------------------------------------------------
// Known value roundtrips via single_array_xml builder
// ---------------------------------------------------------------------------

#[test]
fn known_f64_values_via_xml_roundtrip() {
    let values = vec![1.0_f64, 2.0, 3.0, 100.5, -42.0];
    let binary = BinaryData::F64(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Float64, &binary, None);
    let result = parse_single_array(&xml).expect("should have binary data");
    let got = result.to_f64_vec();
    assert_eq!(got, values);
}

#[test]
fn known_f32_values_via_xml_roundtrip() {
    let values = vec![1.0_f32, 2.0, 3.0, 100.5, -42.0];
    let binary = BinaryData::F32(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Float32, &binary, None);
    let result = parse_single_array(&xml).expect("should have binary data");
    match result {
        BinaryData::F32(got) => assert_eq!(got, values),
        BinaryData::F64(got) => {
            // Parser may promote to f64 — compare with promotion
            let expected: Vec<f64> = values.iter().map(|v| *v as f64).collect();
            assert_eq!(got, expected);
        }
        other => panic!("unexpected variant: {}", other.variant_name()),
    }
}

#[test]
fn known_i32_values_via_xml_roundtrip() {
    let values = vec![0_i32, 1, -1, i32::MAX, i32::MIN];
    let binary = BinaryData::I32(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Int32, &binary, None);
    let result = parse_single_array(&xml).expect("should have binary data");
    match result {
        BinaryData::I32(got) => assert_eq!(got, values),
        other => panic!("unexpected variant: {}", other.variant_name()),
    }
}

#[test]
fn known_i64_values_via_xml_roundtrip() {
    let values = vec![0_i64, 1, -1, i64::MAX, i64::MIN];
    let binary = BinaryData::I64(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Int64, &binary, None);
    let result = parse_single_array(&xml).expect("should have binary data");
    match result {
        BinaryData::I64(got) => assert_eq!(got, values),
        other => panic!("unexpected variant: {}", other.variant_name()),
    }
}

#[test]
fn single_element_f64_roundtrip() {
    let values = vec![std::f64::consts::PI];
    let binary = BinaryData::F64(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Float64, &binary, None);
    let result = parse_single_array(&xml).expect("should have binary data");
    let got = result.to_f64_vec();
    assert_eq!(got, values);
}

#[test]
fn empty_base64_produces_empty_array() {
    let values: Vec<f64> = vec![];
    let binary = BinaryData::F64(values);
    let xml = single_array_xml("MS:1000514", NumericType::Float64, &binary, Some(0));
    let result = parse_single_array(&xml);
    // Empty binary may be returned as None or as empty BinaryData
    match result {
        None => {} // acceptable
        Some(bin) => assert_eq!(bin.len(), 0, "empty base64 should give empty array"),
    }
}

#[test]
fn large_array_base64_roundtrip() {
    // 1000 elements — tests that base64 encoding/decoding handles multi-line
    // or long strings correctly.
    let values: Vec<f64> = (0..1000).map(|i| i as f64 * 0.001).collect();
    let binary = BinaryData::F64(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Float64, &binary, None);
    let result = parse_single_array(&xml).expect("should have binary data");
    let got = result.to_f64_vec();
    assert_eq!(got, values);
}

// ---------------------------------------------------------------------------
// Raw base64 edge cases tested via hand-crafted XML
// ---------------------------------------------------------------------------

/// Tests that the parser handles base64 strings with embedded newlines/spaces.
/// Some mzML generators emit whitespace inside <binary> elements.
#[test]
fn base64_with_embedded_whitespace() {
    // Encode [1.0_f64, 2.0_f64] = 16 bytes → 24 base64 chars
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
    let bytes: Vec<u8> = [1.0_f64, 2.0_f64]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let encoded = BASE64_STANDARD.encode(&bytes);
    // Insert whitespace into the base64 string
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

/// Tests that all-zero bytes decode correctly.
#[test]
fn all_zeros_base64_roundtrip() {
    let values = vec![0.0_f64; 10];
    let binary = BinaryData::F64(values.clone());
    let xml = single_array_xml("MS:1000514", NumericType::Float64, &binary, None);
    let result = parse_single_array(&xml).expect("should have binary data");
    let got = result.to_f64_vec();
    assert_eq!(got, values);
}
