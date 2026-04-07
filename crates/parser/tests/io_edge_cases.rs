mod common;

use common::assertions::*;
use common::fixtures;
use ionic::mzml::bin_to_mzml::bin_to_mzml;
use ionic::mzml::parse_mzml::parse_mzml;

// Tests 44-48: IO edge cases ported from pwiz_mzml.rs.

#[test]
fn namespaces_parse_correctly() {
    let m10 = fixtures::tiny_pwiz_10();
    let m11 = fixtures::tiny_pwiz_11();

    assert_eq!(common::spectra(m10).len(), 4);
    assert_eq!(common::spectra(m11).len(), 4);

    assert!(m10
        .cv_list
        .as_ref()
        .map(|c| !c.cv.is_empty())
        .unwrap_or(false));
    assert!(m11
        .cv_list
        .as_ref()
        .map(|c| !c.cv.is_empty())
        .unwrap_or(false));
}

#[test]
fn roundtrip_xml_parse_stability() {
    let src = fixtures::tiny_pwiz_11();
    let xml = bin_to_mzml(src).expect("bin_to_mzml should succeed");

    let reparsed_once = common::parse_xml(&xml);
    let xml2 = bin_to_mzml(&reparsed_once).expect("second bin_to_mzml should succeed");
    let reparsed_twice = common::parse_xml(&xml2);

    assert_mzml_semantic_eq(&reparsed_once, &reparsed_twice);
}

#[test]
fn slim_flag_preserves_identity() {
    // The slim parameter has been removed. This test verifies that parsing
    // the same fixture twice yields structurally equivalent results (i.e.
    // the former full==slim guarantee holds trivially).
    let a = fixtures::tiny_pwiz_11();
    let b = common::parse_fixture("pwiz/example_data/tiny.pwiz.1.1.mzML");

    assert_eq!(a.run.id, b.run.id);
    assert_eq!(
        common::top_level_source_file_ids(a),
        common::top_level_source_file_ids(&b)
    );
    assert_eq!(
        common::top_level_software_ids(a),
        common::top_level_software_ids(&b)
    );

    assert_mzml_structural_eq(a, &b);
}

#[test]
fn minimal_mzml_document_accepted() {
    let xml = r#"
<mzML>
  <fileDescription>
    <fileContent/>
    <sourceFileList count="0"/>
  </fileDescription>
  <run id="minimal"/>
</mzML>
"#;
    let m = common::parse_xml(xml);
    assert!(m.run.spectrum_list.is_none());
    assert!(m.run.chromatogram_list.is_none());
}

#[test]
fn non_mzml_root_is_graceful() {
    let mzxml_like = r#"<mzXML><msRun/></mzXML>"#;
    let parsed = parse_mzml(mzxml_like.as_bytes()).expect("parser should be graceful");
    assert_eq!(parsed.run.id, "");
    assert!(parsed.run.spectrum_list.is_none());
    assert!(parsed.run.chromatogram_list.is_none());
}
