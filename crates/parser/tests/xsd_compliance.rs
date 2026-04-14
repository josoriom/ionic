mod common;

use common::helpers::{
    default_cv_list_like_writer, full_mzml_all_optional_fields, minimal_data_processing_list,
    minimal_file_description, minimal_instrument_list, minimal_software_list,
    synthetic_binary_data_array,
};
use ionic::mzml::{bin_to_mzml::convert_bin_to_mzml_bytes, structs::*};

const MZML_CHILD_ORDER: &[&str] = &[
    "cvList",
    "fileDescription",
    "referenceableParamGroupList",
    "sampleList",
    "softwareList",
    "scanSettingsList",
    "instrumentConfigurationList",
    "dataProcessingList",
    "run",
];

fn extract_mzml_child_positions(xml: &str) -> Vec<(&'static str, usize)> {
    MZML_CHILD_ORDER
        .iter()
        .filter_map(|&name| {
            let needle = format!("<{name}");
            xml.find(&needle).map(|pos| (name, pos))
        })
        .collect()
}

fn assert_xsd_order(positions: &[(&str, usize)]) {
    for w in positions.windows(2) {
        assert!(
            w[0].1 < w[1].1,
            "XSD order violation: <{}> (byte {}) must appear BEFORE <{}> (byte {})",
            w[0].0,
            w[0].1,
            w[1].0,
            w[1].1,
        );
    }
}

#[test]
fn mzml_child_element_order_all_sections() {
    let mzml = full_mzml_all_optional_fields();
    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("serialization should succeed");
    let xml = String::from_utf8(bytes).expect("valid UTF-8");

    let positions = extract_mzml_child_positions(&xml);

    assert_eq!(
        positions.len(),
        MZML_CHILD_ORDER.len(),
        "expected all {} children present, got: {:?}",
        MZML_CHILD_ORDER.len(),
        positions.iter().map(|(n, _)| *n).collect::<Vec<_>>()
    );

    assert_xsd_order(&positions);
}

#[test]
fn mzml_child_order_subset_of_optional_elements() {
    let mzml = MzML {
        cv_list: Some(default_cv_list_like_writer()),
        file_description: Some(minimal_file_description()),
        software_list: Some(minimal_software_list()),
        instrument_list: Some(minimal_instrument_list()),
        data_processing_list: Some(minimal_data_processing_list()),
        run: Run {
            id: "subset-test".to_string(),
            default_instrument_configuration_ref: Some("test-ic".to_string()),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                default_data_processing_ref: Some("test-dp".to_string()),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: Some(2),
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(2),
                        binary_data_arrays: vec![
                            synthetic_binary_data_array(
                                "MS:1000514",
                                NumericType::Float64,
                                BinaryData::F64(vec![100.0, 200.0]),
                                Some(2),
                            ),
                            synthetic_binary_data_array(
                                "MS:1000515",
                                NumericType::Float64,
                                BinaryData::F64(vec![10.0, 20.0]),
                                Some(2),
                            ),
                        ],
                    }),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("serialization should succeed");
    let xml = String::from_utf8(bytes).expect("valid UTF-8");

    let positions = extract_mzml_child_positions(&xml);
    assert_xsd_order(&positions);

    let sw_pos = positions.iter().find(|(n, _)| *n == "softwareList");
    let ic_pos = positions
        .iter()
        .find(|(n, _)| *n == "instrumentConfigurationList");
    assert!(
        sw_pos.is_some() && ic_pos.is_some(),
        "both softwareList and instrumentConfigurationList must be present"
    );
    assert!(
        sw_pos.unwrap().1 < ic_pos.unwrap().1,
        "softwareList must appear before instrumentConfigurationList"
    );
}

#[test]
fn indexed_mzml_contains_file_checksum() {
    let mzml = full_mzml_all_optional_fields();
    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("serialization should succeed");
    let xml = String::from_utf8(bytes).expect("valid UTF-8");

    let fc_open = xml.find("<fileChecksum>");
    let fc_close = xml.find("</fileChecksum>");
    assert!(
        fc_open.is_some() && fc_close.is_some(),
        "output must contain <fileChecksum>...</fileChecksum>"
    );

    let start = fc_open.unwrap() + "<fileChecksum>".len();
    let end = fc_close.unwrap();
    let digest = &xml[start..end];

    assert_eq!(
        digest.len(),
        40,
        "SHA-1 hex digest must be 40 characters, got {} ({:?})",
        digest.len(),
        digest
    );
    assert!(
        digest.chars().all(|c| c.is_ascii_hexdigit()),
        "fileChecksum must be a hex string, got: {digest:?}"
    );

    let ilo_pos = xml
        .find("</indexListOffset>")
        .expect("must have indexListOffset");
    let end_indexed = xml
        .find("</indexedmzML>")
        .expect("must have closing indexedmzML");

    assert!(
        fc_open.unwrap() > ilo_pos,
        "<fileChecksum> must come after </indexListOffset>"
    );
    assert!(
        fc_close.unwrap() < end_indexed,
        "</fileChecksum> must come before </indexedmzML>"
    );
}

#[test]
fn empty_source_file_list_is_omitted() {
    let mzml = MzML {
        cv_list: Some(default_cv_list_like_writer()),
        file_description: Some(FileDescription {
            file_content: FileContent::default(),
            source_file_list: SourceFileList {
                count: Some(0),
                source_file: Vec::new(),
            },
            contacts: Vec::new(),
        }),
        software_list: Some(minimal_software_list()),
        instrument_list: Some(minimal_instrument_list()),
        data_processing_list: Some(minimal_data_processing_list()),
        run: Run {
            id: "empty-sfl-test".to_string(),
            default_instrument_configuration_ref: Some("test-ic".to_string()),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                default_data_processing_ref: Some("test-dp".to_string()),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: Some(2),
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(2),
                        binary_data_arrays: vec![
                            synthetic_binary_data_array(
                                "MS:1000514",
                                NumericType::Float64,
                                BinaryData::F64(vec![100.0, 200.0]),
                                Some(2),
                            ),
                            synthetic_binary_data_array(
                                "MS:1000515",
                                NumericType::Float64,
                                BinaryData::F64(vec![10.0, 20.0]),
                                Some(2),
                            ),
                        ],
                    }),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("serialization should succeed");
    let xml = String::from_utf8(bytes).expect("valid UTF-8");

    assert!(
        !xml.contains("<sourceFileList"),
        "empty sourceFileList must NOT be emitted — XSD requires at least one sourceFile child"
    );
}

#[test]
fn nonempty_source_file_list_is_emitted() {
    let mzml = MzML {
        cv_list: Some(default_cv_list_like_writer()),
        file_description: Some(FileDescription {
            file_content: FileContent::default(),
            source_file_list: SourceFileList {
                count: Some(1),
                source_file: vec![SourceFile {
                    id: "sf-1".to_string(),
                    name: "test.raw".to_string(),
                    location: "file:///tmp".to_string(),
                    ..Default::default()
                }],
            },
            contacts: Vec::new(),
        }),
        software_list: Some(minimal_software_list()),
        instrument_list: Some(minimal_instrument_list()),
        data_processing_list: Some(minimal_data_processing_list()),
        run: Run {
            id: "nonempty-sfl-test".to_string(),
            default_instrument_configuration_ref: Some("test-ic".to_string()),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                default_data_processing_ref: Some("test-dp".to_string()),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: Some(2),
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(2),
                        binary_data_arrays: vec![
                            synthetic_binary_data_array(
                                "MS:1000514",
                                NumericType::Float64,
                                BinaryData::F64(vec![100.0, 200.0]),
                                Some(2),
                            ),
                            synthetic_binary_data_array(
                                "MS:1000515",
                                NumericType::Float64,
                                BinaryData::F64(vec![10.0, 20.0]),
                                Some(2),
                            ),
                        ],
                    }),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("serialization should succeed");
    let xml = String::from_utf8(bytes).expect("valid UTF-8");

    assert!(
        xml.contains("<sourceFileList"),
        "non-empty sourceFileList MUST be emitted"
    );
    assert!(
        xml.contains("<sourceFile "),
        "sourceFile element must be present inside the list"
    );
}

#[test]
fn file_checksum_is_valid_sha1() {
    let mzml = full_mzml_all_optional_fields();
    let bytes = convert_bin_to_mzml_bytes(&mzml).expect("serialization should succeed");
    let xml = String::from_utf8(bytes.clone()).expect("valid UTF-8");

    let fc_tag = "<fileChecksum>";
    let fc_open = xml.find(fc_tag).expect("must contain <fileChecksum>");
    let hash_input_end = fc_open + fc_tag.len(); // byte after '>'

    let fc_close = xml.find("</fileChecksum>").expect("must have closing tag");
    let claimed = &xml[hash_input_end..fc_close];

    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(&bytes[..hash_input_end]);
    let computed = format!("{:x}", hasher.finalize());

    assert_eq!(
        claimed, computed,
        "fileChecksum mismatch: claimed {claimed}, computed {computed}"
    );
}
