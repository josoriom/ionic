#![allow(dead_code)]

//! Synthetic MzML builder helpers for the integration test suite.
//!
//! Every function here constructs minimal, well-formed MzML fragments that are
//! useful for property-based and roundtrip tests without requiring on-disk
//! fixture files.

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use ionic::mzml::structs::*;

use super::binary_ext::BinaryDataExt;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// A ready-to-embed `<cvList>` XML fragment matching the writer's default output.
pub const DEFAULT_CV_LIST_XML: &str = concat!(
    "<cvList count=\"2\">",
    "<cv id=\"MS\" fullName=\"Proteomics Standards Initiative Mass Spectrometry Ontology\" version=\"4.1.182\" uri=\"https://raw.githubusercontent.com/HUPO-PSI/psi-ms-CV/master/psi-ms.obo\"/>",
    "<cv id=\"UO\" fullName=\"Unit Ontology\" version=\"09:04:2014\" uri=\"https://raw.githubusercontent.com/bio-ontology-research-group/unit-ontology/master/unit.obo\"/>",
    "</cvList>"
);

// ---------------------------------------------------------------------------
// CvParam helpers
// ---------------------------------------------------------------------------

/// Build a [`CvParam`] with `cvRef="MS"` and the given accession/value.
pub fn synthetic_ms_cv(accession: &str, value: Option<&str>) -> CvParam {
    CvParam {
        cv_ref: Some("MS".to_string()),
        accession: Some(accession.to_string()),
        name: accession.to_string(),
        value: value.map(ToString::to_string),
        ..Default::default()
    }
}

/// Map a [`NumericType`] to the corresponding MS ontology accession for binary
/// precision (e.g. `MS:1000523` for 64-bit float).
pub fn precision_accession(numeric_type: NumericType) -> &'static str {
    match numeric_type {
        NumericType::Float64 => "MS:1000523",
        NumericType::Float32 => "MS:1000521",
        NumericType::Float16 => "MS:1000520",
        NumericType::Int64 => "MS:1000522",
        NumericType::Int32 => "MS:1000519",
        NumericType::Int16 => "MS:1000518",
    }
}

// ---------------------------------------------------------------------------
// Binary data array builders
// ---------------------------------------------------------------------------

/// Build a [`BinaryDataArray`] carrying `binary` with the given role accession,
/// numeric precision, and an optional explicit `arrayLength` override.
pub fn synthetic_binary_data_array(
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
            synthetic_ms_cv("MS:1000576", None), // no compression
        ],
        numeric_type: Some(numeric_type),
        binary: Some(binary),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Top-level structural helpers
// ---------------------------------------------------------------------------

/// Build a minimal [`FileDescription`] with an empty file-content and no source
/// files — just enough for a syntactically valid mzML document.
pub fn minimal_file_description() -> FileDescription {
    FileDescription {
        file_content: FileContent::default(),
        source_file_list: SourceFileList {
            count: Some(0),
            source_file: Vec::new(),
        },
        contacts: Vec::new(),
    }
}

/// Build the standard two-entry [`CvList`] (MS + UO) matching the writer's
/// default output.
pub fn default_cv_list_like_writer() -> CvList {
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

// ---------------------------------------------------------------------------
// Full MzML builders
// ---------------------------------------------------------------------------

/// Build a complete [`MzML`] with one spectrum and one chromatogram, both
/// carrying the same `numeric_type` binary payloads.
///
/// If `declared_length` is `None`, the actual element count of each
/// `BinaryData` payload is used as `defaultArrayLength`.
pub fn synthetic_numeric_matrix_mzml(
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

// ---------------------------------------------------------------------------
// XML string builders
// ---------------------------------------------------------------------------

/// Build a minimal mzML XML string with a single binary data array for testing
/// the XML parser directly (no file on disk required).
pub fn single_array_xml(
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

// ---------------------------------------------------------------------------
// Mutation helpers
// ---------------------------------------------------------------------------

/// Ensure that the first [`Product`] exists on `s`, creating it if necessary.
///
/// This handles the dual-path layout where product data may live either under
/// `spectrum.spectrum_description.product_list` (mzML 1.0) or directly under
/// `spectrum.product_list` (mzML 1.1+).
pub fn ensure_first_product_mut(s: &mut Spectrum) -> &mut Product {
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

/// Ensure that a [`ReferenceableParamGroup`] with the given `id` exists in the
/// mzML's referenceable param group list.
///
/// If the group doesn't exist yet it is created with a single `ms level = 1`
/// cvParam — a sensible default for most test scenarios.
pub fn ensure_referenceable_param_group(mzml: &mut MzML, id: &str) {
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
