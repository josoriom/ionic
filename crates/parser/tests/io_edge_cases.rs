mod common;

use common::assertions::*;
use common::test_files;
use ionic::mzml::{bin_to_mzml::bin_to_mzml, parse_mzml::parse_mzml};

#[test]
fn namespaces_parse_correctly() {
    let m10 = test_files::tiny_pwiz_10();
    let m11 = test_files::tiny_pwiz_11();

    assert_eq!(common::spectra(m10).len(), 4);
    assert_eq!(common::spectra(m11).len(), 4);

    assert!(
        m10.cv_list
            .as_ref()
            .map(|c| !c.cv.is_empty())
            .unwrap_or(false)
    );
    assert!(
        m11.cv_list
            .as_ref()
            .map(|c| !c.cv.is_empty())
            .unwrap_or(false)
    );
}

#[test]
fn roundtrip_xml_parse_stability() {
    let src = test_files::tiny_pwiz_11();
    let xml = bin_to_mzml(src).expect("bin_to_mzml should succeed");

    let reparsed_once = parse_mzml(&xml).expect("first reparse should succeed");
    let xml2 = bin_to_mzml(&reparsed_once).expect("second bin_to_mzml should succeed");
    let reparsed_twice = parse_mzml(&xml2).expect("second reparse should succeed");

    assert_mzml_semantic_eq(&reparsed_once, &reparsed_twice);
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
