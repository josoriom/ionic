//! Check the mzML parser with truncated documents, missing closing tags,
//! invalid nesting, and other malformed inputs to ensure errors are returned (no panics).
mod common;

use ionic::mzml::parse_mzml::{parse_indexed_mzml, parse_mzml};

// Truncated documents (should trigger UnexpectedEof or Xml error)
#[test]
fn truncated_after_mzml_open_tag() {
    let xml = b"<mzML>";
    let err = parse_mzml(xml).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("unexpected end of file") || msg.contains("XML error"),
        "expected EOF-related error, got: {msg}"
    );
}

#[test]
fn truncated_inside_run_element() {
    let xml = br#"<mzML><fileDescription><fileContent/><sourceFileList count="0"/></fileDescription><run id="r">"#;
    let err = parse_mzml(xml).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("unexpected end of file") || msg.contains("XML error"),
        "expected EOF-related error, got: {msg}"
    );
}

#[test]
fn truncated_inside_spectrum_list() {
    let xml = br#"<mzML><fileDescription><fileContent/><sourceFileList count="0"/></fileDescription><run id="r"><spectrumList count="1"><spectrum index="0" id="s1">"#;
    let err = parse_mzml(xml).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("unexpected end of file") || msg.contains("XML error"),
        "expected EOF-related error, got: {msg}"
    );
}

#[test]
fn truncated_mid_attribute() {
    // XML is cut in the middle of an attribute value
    let xml = br#"<mzML><run id="trunc"#;
    let err = parse_mzml(xml).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("unexpected end of file") || msg.contains("XML error"),
        "expected error, got: {msg}"
    );
}

#[test]
fn truncated_indexed_mzml() {
    let xml = b"<indexedmzML><mzML>";
    let err = parse_indexed_mzml(xml).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("unexpected end of file") || msg.contains("XML error"),
        "expected EOF-related error, got: {msg}"
    );
}

// Missing closing tags
#[test]
fn missing_closing_mzml_tag() {
    let xml = br#"<mzML><fileDescription><fileContent/><sourceFileList count="0"/></fileDescription><run id="r"></run>"#;
    // Missing </mzML>
    let err = parse_mzml(xml).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("unexpected end of file") || msg.contains("XML error"),
        "expected EOF-related error, got: {msg}"
    );
}

#[test]
fn missing_closing_run_tag() {
    let xml = br#"<mzML><fileDescription><fileContent/><sourceFileList count="0"/></fileDescription><run id="r"></mzML>"#;
    // </run> is missing — parser should encounter </mzML> while still inside <run>
    // This should either succeed (if the parser is lenient) or return an Xml error
    let result = parse_mzml(xml);
    // We don't mandate success or failure here; just no panic. But if it fails, it
    // should be a proper error.
    if let Err(e) = result {
        let msg = format!("{e}");
        assert!(
            msg.contains("XML error") || msg.contains("unexpected"),
            "expected descriptive error, got: {msg}"
        );
    }
}

// Totally invalid XML
#[test]
fn not_xml_at_all_returns_error_or_default() {
    let garbage = b"this is not xml at all {{{ >>> <<<";
    let result = parse_mzml(garbage);
    // Should return either Ok(default MzML) or Err, but NEVER panic.
    if let Err(e) = result {
        let msg = format!("{e}");
        assert!(
            msg.contains("XML error"),
            "expected XML parse error, got: {msg}"
        );
    }
}

#[test]
fn binary_garbage_returns_error_or_default() {
    let garbage: Vec<u8> = (0..256).map(|i| i as u8).collect();
    let result = parse_mzml(&garbage);
    if let Err(e) = result {
        let _ = format!("{e}");
    }
}

#[test]
fn empty_string_returns_default() {
    let result = parse_mzml(b"").expect("empty input should return default MzML");
    assert_eq!(result.run.id, "");
    assert!(result.run.spectrum_list.is_none());
}

// Deeply nested but valid-looking XML without mzML root
#[test]
fn non_mzml_xml_returns_default() {
    let xml = b"<html><body><p>Hello world</p></body></html>";
    let result = parse_mzml(xml).expect("non-mzML XML should parse gracefully");
    assert_eq!(result.run.id, "");
    assert!(result.run.spectrum_list.is_none());
}

// XML with BOM / whitespace preamble
#[test]
fn xml_with_utf8_bom_does_not_panic() {
    let mut xml = Vec::new();
    xml.extend_from_slice(b"\xEF\xBB\xBF");
    xml.extend_from_slice(br#"<mzML><fileDescription><fileContent/><sourceFileList count="0"/></fileDescription><run id="bom-test"></run></mzML>"#);
    let result = parse_mzml(&xml);
    // The parser may or may not handle BOM gracefully.
    if let Ok(m) = &result {
        assert!(
            m.run.id == "bom-test" || m.run.id.is_empty(),
            "unexpected run id: {}",
            m.run.id
        );
    }
    // An Err is also acceptable — just not a panic.
}

#[test]
fn xml_with_leading_whitespace_does_not_panic() {
    let xml = br#"   
    <mzML><fileDescription><fileContent/><sourceFileList count="0"/></fileDescription><run id="ws-test"></run></mzML>"#;
    let result = parse_mzml(xml);
    assert!(
        result.is_ok(),
        "leading whitespace should not cause an error"
    );
    let m = result.unwrap();
    assert!(
        m.run.id == "ws-test" || m.run.id.is_empty(),
        "unexpected run id: {}",
        m.run.id
    );
}

// Mismatched tags
#[test]
fn mismatched_open_close_tags_returns_error() {
    let xml = b"<mzML><fileDescription></run></mzML>";
    let result = parse_mzml(xml);
    if let Err(e) = result {
        let msg = format!("{e}");
        assert!(
            msg.contains("XML error"),
            "expected XML mismatch error, got: {msg}"
        );
    }
}

// Duplicate elements (should not panic)
#[test]
fn duplicate_run_elements_no_panic() {
    // Use non-self-closing <run> elements so the parser sees Start events.
    let xml = br#"<mzML>
        <fileDescription><fileContent/><sourceFileList count="0"/></fileDescription>
        <run id="first"></run>
        <run id="second"></run>
    </mzML>"#;
    let result = parse_mzml(xml);
    assert!(result.is_ok(), "duplicate runs should not cause a panic");
    let m = result.unwrap();
    assert!(m.run.id == "first" || m.run.id == "second");
}

#[test]
fn absurd_count_attribute_does_not_oom() {
    let xml = br#"<mzML>
        <fileDescription><fileContent/><sourceFileList count="0"/></fileDescription>
        <run id="big"><spectrumList count="999999999"/></run>
    </mzML>"#;
    let result = parse_mzml(xml);
    assert!(result.is_ok(), "large count attribute should not cause OOM");
    let m = result.unwrap();
    let spectra = m
        .run
        .spectrum_list
        .as_ref()
        .map_or(0, |sl| sl.spectra.len());
    assert_eq!(spectra, 0, "no actual spectra should be parsed");
}
