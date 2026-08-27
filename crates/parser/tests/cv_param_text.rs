mod common;

use std::borrow::Cow;

use common::{
    decode_ion, encode_to_ion,
    helpers::{build_mzml, make_spectrum_f64},
    parse_xml, spectra,
};
use ionic::mzml::structs::CvParam;

const MS_LEVEL: &str = "MS:1000511";
const SCAN_TIME: &str = "MS:1000016";

fn param_by_accession<'a>(params: &'a [CvParam], accession: &str) -> &'a CvParam {
    params
        .iter()
        .find(|param| param.accession.as_deref() == Some(accession))
        .unwrap_or_else(|| panic!("no cv param with accession {accession}"))
}

#[test]
fn new_borrows_constant_text() {
    let param = CvParam::new("MS", MS_LEVEL, "ms level");

    assert!(matches!(param.cv_ref, Some(Cow::Borrowed(_))));
    assert!(matches!(param.accession, Some(Cow::Borrowed(_))));
    assert!(matches!(param.name, Cow::Borrowed(_)));
    assert_eq!(param.cv_ref.as_deref(), Some("MS"));
    assert_eq!(param.accession.as_deref(), Some(MS_LEVEL));
    assert_eq!(param.name, "ms level");
    assert!(param.value.is_none());
}

#[test]
fn new_allows_computed_text() {
    let accession = format!("MS:{:07}", 1_000_511);
    let param = CvParam::new("MS", accession, String::from("ms level"));

    assert!(matches!(param.accession, Some(Cow::Owned(_))));
    assert_eq!(param.accession.as_deref(), Some(MS_LEVEL));
    assert_eq!(param.name, "ms level");
}

#[test]
fn with_value_sets_the_value() {
    let param = CvParam::new("MS", MS_LEVEL, "ms level").with_value("1");

    assert_eq!(param.value.as_deref(), Some("1"));
    assert!(matches!(param.value, Some(Cow::Borrowed(_))));
}

#[test]
fn with_unit_sets_every_unit_field() {
    let param = CvParam::new("MS", SCAN_TIME, "scan start time")
        .with_value(1.5.to_string())
        .with_unit("UO", "UO:0000031", "minute");

    assert_eq!(param.unit_cv_ref.as_deref(), Some("UO"));
    assert_eq!(param.unit_accession.as_deref(), Some("UO:0000031"));
    assert_eq!(param.unit_name.as_deref(), Some("minute"));
    assert_eq!(param.value.as_deref(), Some("1.5"));
    assert!(matches!(param.value, Some(Cow::Owned(_))));
}

#[test]
fn json_keeps_the_same_shape() {
    let param = CvParam::new("MS", MS_LEVEL, "ms level").with_value("1");
    let text = serde_json::to_string(&param).expect("cv param serializes");

    assert_eq!(
        text,
        r#"{"cv_ref":"MS","accession":"MS:1000511","name":"ms level","value":"1","unit_cv_ref":null,"unit_name":null,"unit_accession":null}"#
    );

    let back: CvParam = serde_json::from_str(&text).expect("cv param deserializes");
    assert_eq!(
        serde_json::to_string(&back).expect("cv param serializes again"),
        text
    );
}

#[test]
fn empty_param_keeps_the_same_shape() {
    let text = serde_json::to_string(&CvParam::default()).expect("cv param serializes");

    assert_eq!(
        text,
        r#"{"cv_ref":null,"accession":null,"name":"","value":null,"unit_cv_ref":null,"unit_name":null,"unit_accession":null}"#
    );
}

#[test]
fn ion_roundtrip_keeps_known_text() {
    let mut spectrum = make_spectrum_f64("scan=1", vec![100.0, 200.0], vec![10.0, 20.0]);
    spectrum.cv_params = vec![
        CvParam::new("MS", MS_LEVEL, "ms level").with_value("1"),
        CvParam::new("MS", SCAN_TIME, "scan start time")
            .with_value("12.5")
            .with_unit("UO", "UO:0000031", "minute"),
    ];

    let mzml = build_mzml(vec![spectrum], Vec::new());
    let decoded = decode_ion(&encode_to_ion(&mzml, 0, false)).expect("ion decodes");
    let params = &spectra(&decoded)[0].cv_params;

    let level = param_by_accession(params, MS_LEVEL);
    assert_eq!(level.cv_ref.as_deref(), Some("MS"));
    assert_eq!(level.name, "ms level");
    assert_eq!(level.value.as_deref(), Some("1"));

    let time = param_by_accession(params, SCAN_TIME);
    assert_eq!(time.name, "scan start time");
    assert_eq!(time.value.as_deref(), Some("12.5"));
    assert_eq!(time.unit_cv_ref.as_deref(), Some("UO"));
    assert_eq!(time.unit_accession.as_deref(), Some("UO:0000031"));
    assert_eq!(time.unit_name.as_deref(), Some("minute"));
}

#[test]
fn ion_roundtrip_keeps_text_the_table_does_not_have() {
    let mut spectrum = make_spectrum_f64("scan=1", vec![100.0, 200.0], vec![10.0, 20.0]);
    spectrum.cv_params = vec![CvParam::new("MS", "MS:9999999", "vendor term").with_value("7")];

    let mzml = build_mzml(vec![spectrum], Vec::new());
    let decoded = decode_ion(&encode_to_ion(&mzml, 0, false)).expect("ion decodes");
    let params = &spectra(&decoded)[0].cv_params;

    let vendor = param_by_accession(params, "MS:9999999");
    assert_eq!(vendor.cv_ref.as_deref(), Some("MS"));
    assert_eq!(vendor.value.as_deref(), Some("7"));
    assert_eq!(vendor.name, "MS:9999999");
}

#[test]
fn mzml_keeps_text_the_table_does_not_have() {
    let xml = r#"
<mzML>
  <fileDescription>
    <fileContent/>
    <sourceFileList count="0"/>
  </fileDescription>
  <run id="test-run">
    <spectrumList count="1">
      <spectrum index="0" id="scan=1" defaultArrayLength="0">
        <cvParam cvRef="MS" accession="MS:1000511" name="vendor level text" value="1"/>
        <cvParam cvRef="VENDOR" accession="VENDOR:0001" name="vendor term" value="7"/>
      </spectrum>
    </spectrumList>
  </run>
</mzML>
"#;

    let mzml = parse_xml(xml);
    let params = &spectra(&mzml)[0].cv_params;

    let level = param_by_accession(params, MS_LEVEL);
    assert_eq!(level.name, "vendor level text");
    assert!(matches!(level.name, Cow::Owned(_)));
    assert!(matches!(level.accession, Some(Cow::Borrowed(_))));

    let vendor = param_by_accession(params, "VENDOR:0001");
    assert_eq!(vendor.cv_ref.as_deref(), Some("VENDOR"));
    assert_eq!(vendor.name, "vendor term");
    assert!(matches!(vendor.cv_ref, Some(Cow::Owned(_))));
}
