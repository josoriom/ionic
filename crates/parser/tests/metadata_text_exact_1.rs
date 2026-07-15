mod common;

use common::{
    decode_ion, encode_to_ion,
    helpers::{build_mzml, make_spectrum_f64},
};
use ionic::mzml::structs::CvParam;

#[test]
fn dirty_decimal_cv_param_value_is_byte_exact_1() {
    let mut spectrum = make_spectrum_f64("scan=1", vec![100.0, 200.0], vec![10.0, 20.0]);

    spectrum.cv_params = vec![
        CvParam {
            cv_ref: Some("MS".to_string()),
            accession: Some("MS:1000504".to_string()),
            name: "base peak m/z".to_string(),
            value: Some("1003.5599999999999".to_string()),
            ..Default::default()
        },
        CvParam {
            cv_ref: Some("MS".to_string()),
            accession: Some("MS:1000505".to_string()),
            name: "base peak intensity".to_string(),
            value: Some("142.38999999999999".to_string()),
            ..Default::default()
        },
        CvParam {
            cv_ref: Some("MS".to_string()),
            accession: Some("MS:1000285".to_string()),
            name: "total ion current".to_string(),
            value: Some("5.8905000000000003".to_string()),
            ..Default::default()
        },
        CvParam {
            cv_ref: Some("MS".to_string()),
            accession: Some("MS:1000527".to_string()),
            name: "highest observed m/z".to_string(),
            value: Some("1003.56".to_string()),
            ..Default::default()
        },
    ];

    let mzml = build_mzml(vec![spectrum], vec![]);
    let bytes = encode_to_ion(&mzml, 12, false);
    let decoded = decode_ion(&bytes).expect("decode should succeed");

    let params = &decoded.run.spectrum_list.as_ref().unwrap().spectra[0].cv_params;

    let value_for = |accession: &str| -> &str {
        params
            .iter()
            .find(|p| p.accession.as_deref() == Some(accession))
            .unwrap_or_else(|| panic!("missing cv param {accession}"))
            .value
            .as_deref()
            .unwrap()
    };

    assert_eq!(value_for("MS:1000504"), "1003.5599999999999");
    assert_eq!(value_for("MS:1000505"), "142.38999999999999");
    assert_eq!(value_for("MS:1000285"), "5.8905000000000003");
    assert_eq!(value_for("MS:1000527"), "1003.56");
}
