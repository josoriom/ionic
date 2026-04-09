#![allow(dead_code)]

use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use ionic::mzml::{parse_mzml::parse_mzml, structs::*};

use super::binary_ext::BinaryDataExt;

pub const DEFAULT_CV_LIST_XML: &str = concat!(
    "<cvList count=\"2\">",
    "<cv id=\"MS\" fullName=\"Proteomics Standards Initiative Mass Spectrometry Ontology\" version=\"4.1.182\" uri=\"https://raw.githubusercontent.com/HUPO-PSI/psi-ms-CV/master/psi-ms.obo\"/>",
    "<cv id=\"UO\" fullName=\"Unit Ontology\" version=\"09:04:2014\" uri=\"https://raw.githubusercontent.com/bio-ontology-research-group/unit-ontology/master/unit.obo\"/>",
    "</cvList>"
);

pub(crate) fn synthetic_ms_cv(accession: &str, value: Option<&str>) -> CvParam {
    CvParam {
        cv_ref: Some("MS".to_string()),
        accession: Some(accession.to_string()),
        name: accession.to_string(),
        value: value.map(ToString::to_string),
        ..Default::default()
    }
}

pub(crate) fn precision_accession(numeric_type: NumericType) -> &'static str {
    match numeric_type {
        NumericType::Float64 => "MS:1000523",
        NumericType::Float32 => "MS:1000521",
        NumericType::Float16 => "MS:1000520",
        NumericType::Int64 => "MS:1000522",
        NumericType::Int32 => "MS:1000519",
        NumericType::Int16 => "MS:1000518",
    }
}

pub(crate) fn synthetic_binary_data_array(
    role_accession: &str,
    numeric_type: NumericType,
    binary: BinaryData,
    declared_length: Option<usize>,
) -> BinaryDataArray {
    BinaryDataArray {
        array_length: declared_length,
        cv_params: vec![
            synthetic_ms_cv(role_accession, None),
            synthetic_ms_cv(precision_accession(numeric_type), None),
            synthetic_ms_cv("MS:1000576", None),
        ],
        numeric_type: Some(numeric_type),
        binary: Some(binary),
        ..Default::default()
    }
}

pub(crate) fn minimal_file_description() -> FileDescription {
    FileDescription {
        file_content: FileContent::default(),
        source_file_list: SourceFileList {
            count: Some(0),
            source_file: Vec::new(),
        },
        contacts: Vec::new(),
    }
}

pub(crate) fn default_cv_list_like_writer() -> CvList {
    CvList {
        count: Some(2),
        cv: vec![
            CvEntry {
                id: "MS".to_string(),
                full_name: Some(
                    "Proteomics Standards Initiative Mass Spectrometry Ontology".to_string(),
                ),
                version: Some("4.1.182".to_string()),
                uri: Some(
                    "https://raw.githubusercontent.com/HUPO-PSI/psi-ms-CV/master/psi-ms.obo"
                        .to_string(),
                ),
            },
            CvEntry {
                id: "UO".to_string(),
                full_name: Some("Unit Ontology".to_string()),
                version: Some("09:04:2014".to_string()),
                uri: Some(
                    "https://raw.githubusercontent.com/bio-ontology-research-group/unit-ontology/master/unit.obo"
                        .to_string(),
                ),
            },
        ],
    }
}

pub(crate) fn synthetic_numeric_matrix_mzml(
    numeric_type: NumericType,
    spectrum_binary: BinaryData,
    chromatogram_binary: BinaryData,
    declared_length: Option<usize>,
) -> MzML {
    let spectrum_default_array_length = declared_length.or_else(|| Some(spectrum_binary.len()));
    let chromatogram_default_array_length =
        declared_length.or_else(|| Some(chromatogram_binary.len()));

    MzML {
        cv_list: Some(default_cv_list_like_writer()),
        file_description: Some(minimal_file_description()),
        run: Run {
            id: format!("synthetic-{numeric_type:?}"),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: spectrum_default_array_length,
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(2),
                        binary_data_arrays: vec![
                            synthetic_binary_data_array(
                                "MS:1000514",
                                numeric_type,
                                spectrum_binary.clone(),
                                declared_length,
                            ),
                            synthetic_binary_data_array(
                                "MS:1000515",
                                numeric_type,
                                spectrum_binary,
                                declared_length,
                            ),
                        ],
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            chromatogram_list: Some(ChromatogramList {
                count: Some(1),
                chromatograms: vec![Chromatogram {
                    id: format!("chrom-{numeric_type:?}"),
                    index: Some(0),
                    default_array_length: chromatogram_default_array_length,
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(2),
                        binary_data_arrays: vec![
                            synthetic_binary_data_array(
                                "MS:1000595",
                                numeric_type,
                                chromatogram_binary.clone(),
                                declared_length,
                            ),
                            synthetic_binary_data_array(
                                "MS:1000515",
                                numeric_type,
                                chromatogram_binary,
                                declared_length,
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
    }
}

pub(crate) fn single_array_xml(
    role_accession: &str,
    numeric_type: NumericType,
    binary: &BinaryData,
    declared_length: Option<usize>,
) -> String {
    let encoded = BASE64_STANDARD.encode(binary.to_le_bytes());
    let array_length_attr = declared_length
        .map(|value| format!(" arrayLength=\"{value}\""))
        .unwrap_or_default();
    let encoded_length_attr = format!(" encodedLength=\"{}\"", encoded.len());

    format!(
        concat!(
            "<mzML>",
            "<fileDescription><fileContent/><sourceFileList count=\"0\"/></fileDescription>",
            "<run id=\"synthetic\"><spectrumList count=\"1\">",
            "<spectrum index=\"0\" id=\"scan=1\"><binaryDataArrayList count=\"1\">",
            "<binaryDataArray{array_length_attr}{encoded_length_attr}>",
            "<cvParam cvRef=\"MS\" accession=\"{role_accession}\" name=\"role\"/>",
            "<cvParam cvRef=\"MS\" accession=\"{precision_accession}\" name=\"precision\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000576\" name=\"no compression\"/>",
            "<binary>{encoded}</binary>",
            "</binaryDataArray></binaryDataArrayList></spectrum></spectrumList>",
            "</run></mzML>"
        ),
        array_length_attr = array_length_attr,
        encoded_length_attr = encoded_length_attr,
        role_accession = role_accession,
        precision_accession = precision_accession(numeric_type),
        encoded = encoded,
    )
}

pub(crate) fn ensure_first_product_mut(s: &mut Spectrum) -> &mut Product {
    if let Some(sd) = s.spectrum_description.as_mut() {
        let pl = sd.product_list.get_or_insert_with(|| ProductList {
            count: Some(0),
            products: Vec::new(),
            cv_params: Vec::new(),
            user_params: Vec::new(),
        });
        if pl.products.is_empty() {
            pl.products.push(Product::default());
            pl.count = Some(1);
        } else if pl.count.is_none() {
            pl.count = Some(pl.products.len());
        }
        return pl.products.first_mut().expect("first product");
    }

    let pl = s.product_list.get_or_insert_with(|| ProductList {
        count: Some(0),
        products: Vec::new(),
        cv_params: Vec::new(),
        user_params: Vec::new(),
    });
    if pl.products.is_empty() {
        pl.products.push(Product::default());
        pl.count = Some(1);
    } else if pl.count.is_none() {
        pl.count = Some(pl.products.len());
    }
    pl.products.first_mut().expect("first product")
}

pub(crate) fn ensure_referenceable_param_group(mzml: &mut MzML, id: &str) {
    let rpgl =
        mzml.referenceable_param_group_list
            .get_or_insert_with(|| ReferenceableParamGroupList {
                count: Some(0),
                referenceable_param_groups: Vec::new(),
            });

    if rpgl.referenceable_param_groups.iter().any(|g| g.id == id) {
        return;
    }

    rpgl.referenceable_param_groups
        .push(ReferenceableParamGroup {
            id: id.to_string(),
            cv_params: vec![CvParam {
                cv_ref: Some("MS".to_string()),
                accession: Some("MS:1000511".to_string()),
                name: "ms level".to_string(),
                value: Some("1".to_string()),
                unit_cv_ref: None,
                unit_name: None,
                unit_accession: None,
            }],
            user_params: Vec::new(),
        });
    rpgl.count = Some(rpgl.referenceable_param_groups.len());
}

pub(crate) fn mzml_with_single_array(
    numeric_type: NumericType,
    binary: BinaryData,
    len: usize,
) -> MzML {
    MzML {
        file_description: Some(minimal_file_description()),
        run: Run {
            id: format!("array-test-{numeric_type:?}"),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: Some(len),
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(1),
                        binary_data_arrays: vec![synthetic_binary_data_array(
                            "MS:1000514",
                            numeric_type,
                            binary,
                            Some(len),
                        )],
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(crate) fn build_mzml(spectra: Vec<Spectrum>, chromatograms: Vec<Chromatogram>) -> MzML {
    MzML {
        cv_list: Some(default_cv_list_like_writer()),
        file_description: Some(minimal_file_description()),
        run: Run {
            id: "test-run".to_string(),
            spectrum_list: if spectra.is_empty() {
                None
            } else {
                Some(SpectrumList {
                    count: Some(spectra.len()),
                    spectra,
                    ..Default::default()
                })
            },
            chromatogram_list: if chromatograms.is_empty() {
                None
            } else {
                Some(ChromatogramList {
                    count: Some(chromatograms.len()),
                    chromatograms,
                    ..Default::default()
                })
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(crate) fn parse_single_array_xml(xml: &str) -> Option<BinaryData> {
    let mzml = parse_mzml(xml.as_bytes()).expect("parse should succeed");
    let spectra = mzml.run.spectrum_list.as_ref()?.spectra.first()?;
    let bda = spectra
        .binary_data_array_list
        .as_ref()?
        .binary_data_arrays
        .first()?;
    bda.binary.clone()
}

pub(crate) fn make_spectrum_f64(id: &str, mz: Vec<f64>, intensity: Vec<f64>) -> Spectrum {
    let len = mz.len();
    Spectrum {
        id: id.to_string(),
        index: Some(0),
        default_array_length: Some(len),
        binary_data_array_list: Some(BinaryDataArrayList {
            count: Some(2),
            binary_data_arrays: vec![
                synthetic_binary_data_array(
                    "MS:1000514",
                    NumericType::Float64,
                    BinaryData::F64(mz),
                    Some(len),
                ),
                synthetic_binary_data_array(
                    "MS:1000515",
                    NumericType::Float64,
                    BinaryData::F64(intensity),
                    Some(len),
                ),
            ],
        }),
        ..Default::default()
    }
}

pub(crate) fn make_chromatogram_f64(id: &str, time: Vec<f64>, intensity: Vec<f64>) -> Chromatogram {
    let len = time.len();
    Chromatogram {
        id: id.to_string(),
        index: Some(0),
        default_array_length: Some(len),
        binary_data_array_list: Some(BinaryDataArrayList {
            count: Some(2),
            binary_data_arrays: vec![
                synthetic_binary_data_array(
                    "MS:1000595",
                    NumericType::Float64,
                    BinaryData::F64(time),
                    Some(len),
                ),
                synthetic_binary_data_array(
                    "MS:1000515",
                    NumericType::Float64,
                    BinaryData::F64(intensity),
                    Some(len),
                ),
            ],
        }),
        ..Default::default()
    }
}

pub fn minimal_software_list() -> SoftwareList {
    SoftwareList {
        count: Some(1),
        software: vec![Software {
            id: "test-sw".to_string(),
            version: Some("1.0".to_string()),
            ..Default::default()
        }],
    }
}

pub fn minimal_instrument_list() -> InstrumentList {
    InstrumentList {
        count: Some(1),
        instrument: vec![Instrument {
            id: "test-ic".to_string(),
            ..Default::default()
        }],
    }
}

pub fn minimal_scan_settings_list() -> ScanSettingsList {
    ScanSettingsList {
        count: Some(1),
        scan_settings: vec![ScanSettings {
            id: Some("test-ss".to_string()),
            ..Default::default()
        }],
    }
}

pub fn minimal_data_processing_list() -> DataProcessingList {
    DataProcessingList {
        count: Some(1),
        data_processing: vec![DataProcessing {
            id: "test-dp".to_string(),
            software_ref: Some("test-sw".to_string()),
            processing_method: vec![ProcessingMethod {
                order: Some(0),
                software_ref: Some("test-sw".to_string()),
                ..Default::default()
            }],
        }],
    }
}

pub fn full_mzml_all_optional_fields() -> MzML {
    MzML {
        cv_list: Some(default_cv_list_like_writer()),
        file_description: Some(minimal_file_description()),
        referenceable_param_group_list: Some(ReferenceableParamGroupList {
            count: Some(1),
            referenceable_param_groups: vec![ReferenceableParamGroup {
                id: "test-rpg".to_string(),
                cv_params: vec![synthetic_ms_cv("MS:1000511", Some("1"))],
                user_params: Vec::new(),
            }],
        }),
        sample_list: Some(SampleList {
            count: Some(1),
            samples: vec![Sample {
                id: "test-sample".to_string(),
                name: "test".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        software_list: Some(minimal_software_list()),
        scan_settings_list: Some(minimal_scan_settings_list()),
        instrument_list: Some(minimal_instrument_list()),
        data_processing_list: Some(minimal_data_processing_list()),
        run: Run {
            id: "test-run".to_string(),
            default_instrument_configuration_ref: Some("test-ic".to_string()),
            spectrum_list: Some(SpectrumList {
                count: Some(1),
                default_data_processing_ref: Some("test-dp".to_string()),
                spectra: vec![Spectrum {
                    id: "scan=1".to_string(),
                    index: Some(0),
                    default_array_length: Some(3),
                    binary_data_array_list: Some(BinaryDataArrayList {
                        count: Some(2),
                        binary_data_arrays: vec![
                            synthetic_binary_data_array(
                                "MS:1000514",
                                NumericType::Float64,
                                BinaryData::F64(vec![100.0, 200.0, 300.0]),
                                Some(3),
                            ),
                            synthetic_binary_data_array(
                                "MS:1000515",
                                NumericType::Float64,
                                BinaryData::F64(vec![10.0, 20.0, 30.0]),
                                Some(3),
                            ),
                        ],
                    }),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}
