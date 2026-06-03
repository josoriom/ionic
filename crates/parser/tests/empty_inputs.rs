mod common;

use ionic::ion::{Decoder, DecoderConfig, encode};
use ionic::mzml::{
    bin_to_mzml::bin_to_mzml,
    parse_mzml::{parse_indexed_mzml, parse_mzml},
    structs::*,
};

#[test]
fn parse_mzml_empty_bytes() {
    let result = parse_mzml(b"");
    let mzml = result.expect("empty input returns default MzML");
    assert_eq!(mzml.run.id, "");
    assert!(mzml.run.spectrum_list.is_none());
    assert!(mzml.run.chromatogram_list.is_none());
}

#[test]
fn parse_mzml_single_byte() {
    let result = parse_mzml(b"<");
    if let Err(e) = &result {
        let _ = format!("{e}");
    }
}

#[test]
fn parse_mzml_null_byte() {
    let result = parse_mzml(&[0x00]);
    if let Err(e) = &result {
        let _ = format!("{e}");
    }
}

#[test]
fn parse_indexed_mzml_empty_bytes() {
    let result = parse_indexed_mzml(b"");
    let indexed = result.expect("empty input returns default indexed MzML");
    assert_eq!(indexed.mzml.run.id, "");
}

#[test]
fn parse_indexed_mzml_minimal() {
    let xml = b"<indexedmzML><mzML></mzML></indexedmzML>";
    let result = parse_indexed_mzml(xml);
    assert!(result.is_ok(), "minimal indexed mzML should parse");
}

#[test]
fn bin_to_mzml_empty_mzml_returns_error() {
    let mzml = MzML::default();
    let result = bin_to_mzml(&mzml);
    assert!(
        result.is_err(),
        "empty MzML without file_description should return error"
    );
}

#[test]
fn bin_to_mzml_minimal_mzml_succeeds() {
    let mzml = MzML {
        file_description: Some(common::helpers::minimal_file_description()),
        run: Run {
            id: "empty-run".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = bin_to_mzml(&mzml);
    assert!(
        result.is_ok(),
        "minimal MzML with file_description should succeed"
    );
    let xml_bytes = result.unwrap();
    let xml_str = std::str::from_utf8(&xml_bytes).expect("output must be valid UTF-8");
    assert!(xml_str.contains("<mzML"), "output should contain <mzML tag");
    assert!(
        xml_str.contains("empty-run"),
        "output should contain the run id"
    );
}

#[test]
fn encode_empty_mzml() {
    let mzml = MzML::default();
    let mut buf = Vec::new();
    let result = encode(&mzml, 0, false, &mut buf);
    assert!(result.is_ok(), "encoding empty MzML should succeed");
    assert!(!buf.is_empty(), "output should not be empty");
}

#[test]
fn encode_mzml_no_arrays() {
    let mzml = MzML {
        file_description: Some(common::helpers::minimal_file_description()),
        run: Run {
            id: "no-arrays".to_string(),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: Some(0),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = Vec::new();
    let result = encode(&mzml, 0, false, &mut buf);
    assert!(
        result.is_ok(),
        "encoding MzML with spectrum but no arrays should succeed"
    );
}

#[test]
fn decoder_open_empty_bytes() {
    let result = Decoder::open(b"", DecoderConfig::default());
    assert!(
        result.is_err(),
        "empty bytes should not be a valid Ion container"
    );
}

#[test]
fn decoder_open_garbage() {
    let garbage = vec![0xDE, 0xAD, 0xBE, 0xEF];
    let result = Decoder::open(&garbage, DecoderConfig::default());
    assert!(
        result.is_err(),
        "garbage should not be a valid Ion container"
    );
}

#[test]
fn decoder_open_too_small() {
    let result = Decoder::open(&[0x01, 0x02, 0x03], DecoderConfig::default());
    assert!(
        result.is_err(),
        "3 bytes should not be a valid Ion container"
    );
}

#[test]
fn roundtrip_empty_mzml() {
    let mzml = MzML {
        file_description: Some(common::helpers::minimal_file_description()),
        run: Run {
            id: "roundtrip-empty".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = Vec::new();
    encode(&mzml, 0, false, &mut buf).expect("encode should succeed");

    let mut decoder = Decoder::open(&buf, DecoderConfig::default()).expect("decoder should open");
    let decoded = decoder.to_mzml().expect("to_mzml should succeed");
    assert_eq!(decoded.run.id, "roundtrip-empty");
}
