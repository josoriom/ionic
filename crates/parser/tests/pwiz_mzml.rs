use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::PathBuf,
    sync::OnceLock,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use ionic::{
    ion::{encode, Decoder, WritingMode},
    mzml::{
        bin_to_mzml::bin_to_mzml,
        parse_mzml::{parse_indexed_mzml, parse_mzml},
        structs::*,
    },
};

const EPS_REL_F64: f64 = 1e-9;
const EPS_REL_F32: f64 = 1e-5;
const FNV64_OFFSET: u64 = 0xcbf29ce484222325;
const FNV64_PRIME: u64 = 0x00000100000001B3;
const DEFAULT_CV_LIST_XML: &str = concat!(
    "<cvList count=\"2\">",
    "<cv id=\"MS\" fullName=\"Proteomics Standards Initiative Mass Spectrometry Ontology\" version=\"4.1.182\" uri=\"https://raw.githubusercontent.com/HUPO-PSI/psi-ms-CV/master/psi-ms.obo\"/>",
    "<cv id=\"UO\" fullName=\"Unit Ontology\" version=\"09:04:2014\" uri=\"https://raw.githubusercontent.com/bio-ontology-research-group/unit-ontology/master/unit.obo\"/>",
    "</cvList>"
);

fn fnv64_update(mut state: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        state ^= *b as u64;
        state = state.wrapping_mul(FNV64_PRIME);
    }
    state
}

fn fnv64_bytes(bytes: &[u8]) -> u64 {
    fnv64_update(FNV64_OFFSET, bytes)
}

fn fnv64_str(s: &str) -> u64 {
    fnv64_bytes(s.as_bytes())
}

fn hash_binary_payload(bin: &BinaryData) -> u64 {
    let mut state = FNV64_OFFSET;
    match bin {
        BinaryData::F64(v) => {
            for x in v {
                state = fnv64_update(state, &x.to_le_bytes());
            }
        }
        BinaryData::F32(v) => {
            for x in v {
                state = fnv64_update(state, &x.to_le_bytes());
            }
        }
        BinaryData::F16(v) => {
            for x in v {
                state = fnv64_update(state, &x.to_le_bytes());
            }
        }
        BinaryData::I64(v) => {
            for x in v {
                state = fnv64_update(state, &x.to_le_bytes());
            }
        }
        BinaryData::I32(v) => {
            for x in v {
                state = fnv64_update(state, &x.to_le_bytes());
            }
        }
        BinaryData::I16(v) => {
            for x in v {
                state = fnv64_update(state, &x.to_le_bytes());
            }
        }
    }
    state
}

fn repo_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root path must be resolvable")
    })
}

fn read_rel(rel: &str) -> Vec<u8> {
    let full = repo_root().join(rel);
    fs::read(&full).unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", full.display()))
}

fn parse_rel(rel: &str, slim: bool) -> MzML {
    let _ = slim;
    let bytes = read_rel(rel);
    parse_mzml(&bytes).unwrap_or_else(|e| panic!("parse_mzml failed for {rel}: {e}"))
}

fn parse_xml(xml: &str, slim: bool) -> MzML {
    let _ = slim;
    parse_mzml(xml.as_bytes()).unwrap_or_else(|e| panic!("parse_mzml(xml) failed: {e}"))
}

fn parse_indexed_rel(rel: &str) -> IndexedmzML {
    let bytes = read_rel(rel);
    parse_indexed_mzml(&bytes)
        .unwrap_or_else(|e| panic!("parse_indexed_mzml failed for {rel}: {e}"))
}

fn encode_bytes(mzml: &MzML, compression_level: u8, force_f32: bool) -> Vec<u8> {
    let mut out = Vec::new();
    encode(
        mzml,
        compression_level,
        force_f32,
        WritingMode::Memory,
        &mut out,
    )
    .expect("encode should succeed");
    out
}

fn decode(bytes: &[u8]) -> Result<MzML, String> {
    let mut decoder = Decoder::open(bytes)?;
    decoder.to_mzml()
}

fn synthetic_ms_cv(accession: &str, value: Option<&str>) -> CvParam {
    CvParam {
        cv_ref: Some("MS".to_string()),
        accession: Some(accession.to_string()),
        name: accession.to_string(),
        value: value.map(ToString::to_string),
        ..Default::default()
    }
}

fn precision_accession(numeric_type: NumericType) -> &'static str {
    match numeric_type {
        NumericType::Float64 => "MS:1000523",
        NumericType::Float32 => "MS:1000521",
        NumericType::Float16 => "MS:1000520",
        NumericType::Int64 => "MS:1000522",
        NumericType::Int32 => "MS:1000519",
        NumericType::Int16 => "MS:1000518",
    }
}

fn binary_to_le_bytes(binary: &BinaryData) -> Vec<u8> {
    match binary {
        BinaryData::F64(values) => values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
        BinaryData::F32(values) => values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
        BinaryData::F16(values) => values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
        BinaryData::I64(values) => values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
        BinaryData::I32(values) => values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
        BinaryData::I16(values) => values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
    }
}

fn synthetic_binary_data_array(
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

fn minimal_file_description() -> FileDescription {
    FileDescription {
        file_content: FileContent::default(),
        source_file_list: SourceFileList {
            count: Some(0),
            source_file: Vec::new(),
        },
        contacts: Vec::new(),
    }
}

fn default_cv_list_like_writer() -> CvList {
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

fn synthetic_numeric_matrix_mzml(
    numeric_type: NumericType,
    spectrum_binary: BinaryData,
    chromatogram_binary: BinaryData,
    declared_length: Option<usize>,
) -> MzML {
    let spectrum_default_array_length =
        declared_length.or_else(|| Some(binary_len(&spectrum_binary)));
    let chromatogram_default_array_length =
        declared_length.or_else(|| Some(binary_len(&chromatogram_binary)));

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

fn single_array_xml(
    role_accession: &str,
    numeric_type: NumericType,
    binary: &BinaryData,
    declared_length: Option<usize>,
) -> String {
    let encoded = BASE64_STANDARD.encode(binary_to_le_bytes(binary));
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

fn assert_semantic_roundtrip_via_xml(src: &MzML, ctx: &str) {
    let xml = bin_to_mzml(src).unwrap_or_else(|e| panic!("bin_to_mzml failed for {ctx}: {e}"));
    let reparsed = parse_xml(&xml, false);
    assert_mzml_semantic_eq(src, &reparsed);
}

fn assert_semantic_roundtrip_via_b000(src: &MzML, compression_level: u8, ctx: &str) {
    let bytes = encode_bytes(src, compression_level, false);
    let decoded = decode(&bytes).unwrap_or_else(|e| panic!("decode failed for {ctx}: {e}"));
    assert_mzml_semantic_eq(src, &decoded);
}

fn assert_semantic_roundtrip_full_pipeline(src: &MzML, compression_level: u8, ctx: &str) {
    let bytes = encode_bytes(src, compression_level, false);
    let decoded = decode(&bytes).unwrap_or_else(|e| panic!("decode failed for {ctx}: {e}"));
    let xml = bin_to_mzml(&decoded).unwrap_or_else(|e| panic!("bin_to_mzml failed for {ctx}: {e}"));
    let reparsed = parse_xml(&xml, false);
    assert_mzml_semantic_eq(src, &reparsed);
}

fn assert_index_offsets_match_model(indexed: &IndexedmzML, ctx: &str) {
    let spectrum_ids: Vec<_> = spectra(&indexed.mzml)
        .iter()
        .map(|s| s.id.as_str().to_string())
        .collect();
    let chromatogram_ids: Vec<_> = chromatograms(&indexed.mzml)
        .iter()
        .map(|c| c.id.as_str().to_string())
        .collect();

    assert_eq!(
        indexed.index_list.spectrum.len(),
        spectrum_ids.len(),
        "{ctx}: indexed spectrum count mismatch"
    );
    assert_eq!(
        indexed.index_list.chromatogram.len(),
        chromatogram_ids.len(),
        "{ctx}: indexed chromatogram count mismatch"
    );

    for (index, (offset, expected_id)) in indexed
        .index_list
        .spectrum
        .iter()
        .zip(spectrum_ids.iter())
        .enumerate()
    {
        assert_eq!(
            offset.id_ref.as_deref(),
            Some(expected_id.as_str()),
            "{ctx}: indexed spectrum id mismatch at {index}"
        );
        assert!(
            offset.offset > 0,
            "{ctx}: indexed spectrum offset {index} is zero"
        );
    }

    for (index, (offset, expected_id)) in indexed
        .index_list
        .chromatogram
        .iter()
        .zip(chromatogram_ids.iter())
        .enumerate()
    {
        assert_eq!(
            offset.id_ref.as_deref(),
            Some(expected_id.as_str()),
            "{ctx}: indexed chromatogram id mismatch at {index}"
        );
        assert!(
            offset.offset > 0,
            "{ctx}: indexed chromatogram offset {index} is zero"
        );
    }

    let mut previous = 0_u64;
    for (index, offset) in indexed.index_list.spectrum.iter().enumerate() {
        assert!(
            offset.offset >= previous,
            "{ctx}: indexed spectrum offsets are not monotonic at {index}"
        );
        previous = offset.offset;
    }
    previous = 0;
    for (index, offset) in indexed.index_list.chromatogram.iter().enumerate() {
        assert!(
            offset.offset >= previous,
            "{ctx}: indexed chromatogram offsets are not monotonic at {index}"
        );
        previous = offset.offset;
    }

    if !indexed.index_list.spectrum.is_empty() || !indexed.index_list.chromatogram.is_empty() {
        assert!(
            indexed.index_list_offset.is_some(),
            "{ctx}: indexListOffset missing despite populated index entries"
        );
    }
}

fn cv_has_accession(cv_params: &[CvParam], accession: &str) -> bool {
    cv_params
        .iter()
        .any(|p| p.accession.as_deref() == Some(accession))
}

fn rel_close_f64(a: f64, b: f64, eps_rel: f64, ctx: &str) {
    let diff = (a - b).abs();
    let scale = a.abs().max(b.abs()).max(1.0);
    assert!(
        diff <= scale * eps_rel,
        "{ctx}: values differ: left={a} right={b} diff={diff} allowed={} (rel={eps_rel})",
        scale * eps_rel
    );
}

fn assert_binary_semantic_eq(left: &BinaryData, right: &BinaryData, ctx: &str) {
    match (left, right) {
        (BinaryData::F64(l), BinaryData::F64(r)) => {
            assert_eq!(l.len(), r.len(), "{ctx}: f64 len mismatch");
            for (i, (lv, rv)) in l.iter().zip(r.iter()).enumerate() {
                rel_close_f64(*lv, *rv, EPS_REL_F64, &format!("{ctx} f64[{i}]"));
            }
        }
        (BinaryData::F32(l), BinaryData::F32(r)) => {
            assert_eq!(l.len(), r.len(), "{ctx}: f32 len mismatch");
            for (i, (lv, rv)) in l.iter().zip(r.iter()).enumerate() {
                rel_close_f64(
                    *lv as f64,
                    *rv as f64,
                    EPS_REL_F32,
                    &format!("{ctx} f32[{i}]"),
                );
            }
        }
        (BinaryData::F16(l), BinaryData::F16(r)) => assert_eq!(l, r, "{ctx}: f16 payload mismatch"),
        (BinaryData::I64(l), BinaryData::I64(r)) => assert_eq!(l, r, "{ctx}: i64 payload mismatch"),
        (BinaryData::I32(l), BinaryData::I32(r)) => assert_eq!(l, r, "{ctx}: i32 payload mismatch"),
        (BinaryData::I16(l), BinaryData::I16(r)) => assert_eq!(l, r, "{ctx}: i16 payload mismatch"),
        (l, r) => panic!("{ctx}: binary variant mismatch: left={l:?} right={r:?}"),
    }
}

fn bda_role(bda: &BinaryDataArray) -> &'static str {
    if cv_has_accession(&bda.cv_params, "MS:1000514") {
        return "mz";
    }
    if cv_has_accession(&bda.cv_params, "MS:1000515") {
        return "intensity";
    }
    if cv_has_accession(&bda.cv_params, "MS:1000595") {
        return "time";
    }
    if cv_has_accession(&bda.cv_params, "MS:1000786") {
        return "non_standard";
    }
    "other"
}

fn spectrum_scan_count(s: &Spectrum) -> usize {
    if let Some(sd) = s.spectrum_description.as_ref() {
        return sd.scan_list.as_ref().map(|sl| sl.scans.len()).unwrap_or(0);
    }
    s.scan_list.as_ref().map(|sl| sl.scans.len()).unwrap_or(0)
}

fn spectrum_precursor_count(s: &Spectrum) -> usize {
    if let Some(sd) = s.spectrum_description.as_ref() {
        return sd
            .precursor_list
            .as_ref()
            .map(|pl| pl.precursors.len())
            .unwrap_or(0);
    }
    s.precursor_list
        .as_ref()
        .map(|pl| pl.precursors.len())
        .unwrap_or(0)
}

fn spectrum_product_count(s: &Spectrum) -> usize {
    if let Some(sd) = s.spectrum_description.as_ref() {
        return sd
            .product_list
            .as_ref()
            .map(|pl| pl.products.len())
            .unwrap_or(0);
    }
    s.product_list
        .as_ref()
        .map(|pl| pl.products.len())
        .unwrap_or(0)
}

fn spectrum_arrays(s: &Spectrum) -> &[BinaryDataArray] {
    s.binary_data_array_list
        .as_ref()
        .map(|b| b.binary_data_arrays.as_slice())
        .unwrap_or(&[])
}

fn chromatogram_arrays(c: &Chromatogram) -> &[BinaryDataArray] {
    c.binary_data_array_list
        .as_ref()
        .map(|b| b.binary_data_arrays.as_slice())
        .unwrap_or(&[])
}

fn spectra(mzml: &MzML) -> &[Spectrum] {
    mzml.run
        .spectrum_list
        .as_ref()
        .map(|list| list.spectra.as_slice())
        .unwrap_or(&[])
}

fn chromatograms(mzml: &MzML) -> &[Chromatogram] {
    mzml.run
        .chromatogram_list
        .as_ref()
        .map(|list| list.chromatograms.as_slice())
        .unwrap_or(&[])
}

fn set_of_ids<T, F>(items: &[T], mut f: F) -> BTreeSet<String>
where
    F: FnMut(&T) -> Option<&str>,
{
    let mut out = BTreeSet::new();
    for item in items {
        if let Some(id) = f(item) {
            out.insert(id.to_string());
        }
    }
    out
}

fn top_level_software_ids(m: &MzML) -> BTreeSet<String> {
    m.software_list
        .as_ref()
        .map(|sl| set_of_ids(&sl.software, |s| Some(s.id.as_str())))
        .unwrap_or_default()
}

fn top_level_dp_ids(m: &MzML) -> BTreeSet<String> {
    m.data_processing_list
        .as_ref()
        .map(|dpl| set_of_ids(&dpl.data_processing, |dp| Some(dp.id.as_str())))
        .unwrap_or_default()
}

fn top_level_source_file_ids(m: &MzML) -> BTreeSet<String> {
    m.file_description
        .as_ref()
        .map(|fd| set_of_ids(&fd.source_file_list.source_file, |sf| Some(sf.id.as_str())))
        .unwrap_or_default()
}

fn top_level_instrument_ids(m: &MzML) -> BTreeSet<String> {
    m.instrument_list
        .as_ref()
        .map(|il| set_of_ids(&il.instrument, |ic| Some(ic.id.as_str())))
        .unwrap_or_default()
}

fn top_level_sample_ids(m: &MzML) -> BTreeSet<String> {
    m.sample_list
        .as_ref()
        .map(|sl| set_of_ids(&sl.samples, |s| Some(s.id.as_str())))
        .unwrap_or_default()
}

struct SemanticCtx<'a> {
    ref_groups: HashMap<&'a str, &'a ReferenceableParamGroup>,
}

impl<'a> SemanticCtx<'a> {
    fn new(mzml: &'a MzML) -> Self {
        let mut ref_groups = HashMap::new();
        if let Some(list) = mzml.referenceable_param_group_list.as_ref() {
            for group in &list.referenceable_param_groups {
                ref_groups.insert(group.id.as_str(), group);
            }
        }
        Self { ref_groups }
    }

    fn effective_param_signatures(
        &self,
        refs: &[ReferenceableParamGroupRef],
        cv_params: &[CvParam],
        user_params: &[UserParam],
    ) -> Vec<String> {
        let mut out = Vec::with_capacity(cv_params.len() + user_params.len() + refs.len());
        for group_ref in refs {
            if let Some(group) = self.ref_groups.get(group_ref.r#ref.as_str()) {
                for cv_param in &group.cv_params {
                    out.push(cv_param_signature(cv_param));
                }
                for user_param in &group.user_params {
                    out.push(user_param_signature(user_param));
                }
            } else {
                out.push(format!("missing-ref-group:{}", group_ref.r#ref));
            }
        }
        for cv_param in cv_params {
            out.push(cv_param_signature(cv_param));
        }
        for user_param in user_params {
            out.push(user_param_signature(user_param));
        }
        out.sort();
        out
    }
}

fn normalized_text(value: Option<&str>) -> Option<&str> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn normalized_owned_text(value: Option<&str>) -> String {
    normalized_text(value).unwrap_or("").to_string()
}

fn canonical_version_text(value: Option<&str>) -> Option<String> {
    normalized_text(value).map(|text| {
        if let Ok(number) = text.parse::<f64>() {
            if number.fract() == 0.0 {
                return format!("{number:.0}");
            }
        }
        text.to_string()
    })
}

fn should_treat_as_numeric_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let unsigned = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);

    if unsigned.chars().all(|ch| ch.is_ascii_digit()) {
        return unsigned.len() <= 15 && trimmed.parse::<f64>().is_ok();
    }

    if unsigned.contains(['.', 'e', 'E']) {
        return trimmed.parse::<f64>().is_ok();
    }

    false
}

fn canonical_value_text(value: Option<&str>) -> Option<String> {
    normalized_text(value).map(|text| {
        if should_treat_as_numeric_text(text) {
            let number = text.parse::<f64>().expect("numeric text must parse");
            let mut formatted = format!("{number:.12}");
            while formatted.contains('.') && formatted.ends_with('0') {
                formatted.pop();
            }
            if formatted.ends_with('.') {
                formatted.pop();
            }
            return formatted;
        }
        text.to_string()
    })
}

fn cv_param_signature(param: &CvParam) -> String {
    let canonical_name = if normalized_text(param.accession.as_deref()).is_some() {
        None
    } else {
        normalized_text(Some(param.name.as_str()))
    };
    let canonical_unit_name = if normalized_text(param.unit_accession.as_deref()).is_some() {
        None
    } else {
        normalized_text(param.unit_name.as_deref())
    };
    let canonical_unit_cv_ref = if normalized_text(param.unit_accession.as_deref()).is_some() {
        None
    } else {
        normalized_text(param.unit_cv_ref.as_deref())
    };
    format!(
        "cv|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        normalized_text(param.cv_ref.as_deref()),
        normalized_text(param.accession.as_deref()),
        canonical_name,
        canonical_value_text(param.value.as_deref()),
        canonical_unit_cv_ref,
        canonical_unit_name,
        normalized_text(param.unit_accession.as_deref())
    )
}

fn user_param_signature(param: &UserParam) -> String {
    format!(
        "user|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        normalized_text(Some(param.name.as_str())),
        normalized_text(param.r#type.as_deref()),
        normalized_text(param.unit_accession.as_deref()),
        normalized_text(param.unit_cv_ref.as_deref()),
        normalized_text(param.unit_name.as_deref()),
        normalized_text(param.value.as_deref())
    )
}

fn sorted_signatures(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = values.into_iter().collect();
    out.sort();
    out
}

fn assert_signature_vec_eq(left: Vec<String>, right: Vec<String>, ctx: &str) {
    assert_eq!(left, right, "{ctx}: semantic signature mismatch");
}

fn assert_effective_params_eq(
    left_ctx: &SemanticCtx<'_>,
    left_refs: &[ReferenceableParamGroupRef],
    left_cv: &[CvParam],
    left_user: &[UserParam],
    right_ctx: &SemanticCtx<'_>,
    right_refs: &[ReferenceableParamGroupRef],
    right_cv: &[CvParam],
    right_user: &[UserParam],
    ctx: &str,
) {
    let left = left_ctx.effective_param_signatures(left_refs, left_cv, left_user);
    let right = right_ctx.effective_param_signatures(right_refs, right_cv, right_user);
    assert_signature_vec_eq(left, right, ctx);
}

fn binary_len(binary: &BinaryData) -> usize {
    match binary {
        BinaryData::F64(v) => v.len(),
        BinaryData::F32(v) => v.len(),
        BinaryData::F16(v) => v.len(),
        BinaryData::I64(v) => v.len(),
        BinaryData::I32(v) => v.len(),
        BinaryData::I16(v) => v.len(),
    }
}

fn binary_semantically_empty(binary: Option<&BinaryData>) -> bool {
    match binary {
        None => true,
        Some(binary) => binary_len(binary) == 0,
    }
}

fn effective_data_processing_ref<'a>(
    raw: Option<&'a str>,
    default_ref: Option<&'a str>,
) -> Option<&'a str> {
    normalized_text(raw).or_else(|| normalized_text(default_ref))
}

fn assert_opt_str_eq(left: Option<&str>, right: Option<&str>, ctx: &str) {
    assert_eq!(normalized_text(left), normalized_text(right), "{ctx}");
}

fn assert_optional_count_eq(label: &str, declared: Option<usize>, actual: usize) {
    if let Some(declared) = declared {
        assert_eq!(declared, actual, "{label}: declared count mismatch");
    }
}

fn assert_optional_count_eq_u32(label: &str, declared: Option<u32>, actual: usize) {
    if let Some(declared) = declared {
        assert_eq!(
            declared as usize, actual,
            "{label}: declared count mismatch"
        );
    }
}

fn source_file_refs_signature(list: Option<&SourceFileRefList>) -> Vec<String> {
    list.map(|list| {
        sorted_signatures(
            list.source_file_refs
                .iter()
                .map(|item| normalized_owned_text(Some(item.r#ref.as_str()))),
        )
    })
    .unwrap_or_default()
}

fn software_param_signature(param: &SoftwareParam) -> String {
    format!(
        "{:?}",
        (
            normalized_text(param.cv_ref.as_deref()),
            normalized_text(Some(param.accession.as_str())),
            None::<&str>,
        )
    )
}

fn source_file_signature(ctx: &SemanticCtx<'_>, source_file: &SourceFile) -> String {
    format!(
        "{:?}",
        (
            normalized_text(Some(source_file.id.as_str())),
            normalized_text(Some(source_file.name.as_str())),
            normalized_text(Some(source_file.location.as_str())),
            ctx.effective_param_signatures(
                &source_file.referenceable_param_group_ref,
                &source_file.cv_param,
                &source_file.user_param,
            ),
        )
    )
}

fn contact_signature(ctx: &SemanticCtx<'_>, contact: &Contact) -> String {
    format!(
        "{:?}",
        ctx.effective_param_signatures(
            &contact.referenceable_param_group_refs,
            &contact.cv_params,
            &contact.user_params,
        )
    )
}

fn sample_signature(ctx: &SemanticCtx<'_>, sample: &Sample) -> String {
    let refs = sample
        .referenceable_param_group_ref
        .as_ref()
        .map(|value| vec![value.clone()])
        .unwrap_or_default();
    format!(
        "{:?}",
        (
            normalized_text(Some(sample.id.as_str())),
            normalized_text(Some(sample.name.as_str())),
            ctx.effective_param_signatures(&refs, &sample.cv_params, &sample.user_params),
        )
    )
}

fn component_signature(
    ctx: &SemanticCtx<'_>,
    kind: &str,
    order: Option<u32>,
    refs: &[ReferenceableParamGroupRef],
    cv_params: &[CvParam],
    user_params: &[UserParam],
) -> String {
    format!(
        "{:?}",
        (
            kind,
            order,
            ctx.effective_param_signatures(refs, cv_params, user_params),
        )
    )
}

fn instrument_signature(ctx: &SemanticCtx<'_>, instrument: &Instrument) -> String {
    let mut components = Vec::new();
    if let Some(component_list) = instrument.component_list.as_ref() {
        for source in &component_list.source {
            components.push(component_signature(
                ctx,
                "source",
                source.order,
                &source.referenceable_param_group_ref,
                &source.cv_param,
                &source.user_param,
            ));
        }
        for analyzer in &component_list.analyzer {
            components.push(component_signature(
                ctx,
                "analyzer",
                analyzer.order,
                &analyzer.referenceable_param_group_ref,
                &analyzer.cv_param,
                &analyzer.user_param,
            ));
        }
        for detector in &component_list.detector {
            components.push(component_signature(
                ctx,
                "detector",
                detector.order,
                &detector.referenceable_param_group_ref,
                &detector.cv_param,
                &detector.user_param,
            ));
        }
    }
    components.sort();

    format!(
        "{:?}",
        (
            normalized_text(Some(instrument.id.as_str())),
            normalized_text(
                instrument
                    .scan_settings_ref
                    .as_ref()
                    .map(|value| value.r#ref.as_str())
            ),
            normalized_text(
                instrument
                    .software_ref
                    .as_ref()
                    .map(|value| value.r#ref.as_str())
            ),
            ctx.effective_param_signatures(
                &instrument.referenceable_param_group_ref,
                &instrument.cv_param,
                &instrument.user_param,
            ),
            components,
        )
    )
}

fn software_signature(software: &Software) -> String {
    let effective_version = canonical_version_text(software.version.as_deref()).or_else(|| {
        software
            .software_param
            .iter()
            .find_map(|param| canonical_version_text(param.version.as_deref()))
    });

    let mut software_params = software
        .software_param
        .iter()
        .map(software_param_signature)
        .collect::<Vec<_>>();
    software_params.sort();

    let mut cv_params = software
        .cv_param
        .iter()
        .map(cv_param_signature)
        .collect::<Vec<_>>();
    cv_params.sort();

    let mut user_params = software
        .user_params
        .iter()
        .map(user_param_signature)
        .collect::<Vec<_>>();
    user_params.sort();

    format!(
        "{:?}",
        (
            normalized_text(Some(software.id.as_str())),
            effective_version,
            software_params,
            cv_params,
            user_params,
        )
    )
}

fn processing_method_signature(ctx: &SemanticCtx<'_>, method: &ProcessingMethod) -> String {
    format!(
        "{:?}",
        (
            method.order,
            normalized_text(method.software_ref.as_deref()),
            ctx.effective_param_signatures(
                &method.referenceable_param_group_ref,
                &method.cv_param,
                &method.user_param,
            ),
        )
    )
}

fn data_processing_signature(ctx: &SemanticCtx<'_>, data_processing: &DataProcessing) -> String {
    let mut methods = data_processing
        .processing_method
        .iter()
        .map(|item| processing_method_signature(ctx, item))
        .collect::<Vec<_>>();
    methods.sort();

    format!(
        "{:?}",
        (
            normalized_text(Some(data_processing.id.as_str())),
            normalized_text(data_processing.software_ref.as_deref()),
            methods,
        )
    )
}

fn target_signature(ctx: &SemanticCtx<'_>, target: &Target) -> String {
    format!(
        "{:?}",
        ctx.effective_param_signatures(
            &target.referenceable_param_group_refs,
            &target.cv_params,
            &target.user_params,
        )
    )
}

fn scan_settings_signature(ctx: &SemanticCtx<'_>, scan_settings: &ScanSettings) -> String {
    let mut targets = scan_settings
        .target_list
        .as_ref()
        .map(|list| {
            list.targets
                .iter()
                .map(|item| target_signature(ctx, item))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    targets.sort();

    format!(
        "{:?}",
        (
            normalized_text(scan_settings.id.as_deref()),
            normalized_text(scan_settings.instrument_configuration_ref.as_deref()),
            ctx.effective_param_signatures(
                &scan_settings.referenceable_param_group_refs,
                &scan_settings.cv_params,
                &scan_settings.user_params,
            ),
            source_file_refs_signature(scan_settings.source_file_ref_list.as_ref()),
            targets,
        )
    )
}

fn run_signature(ctx: &SemanticCtx<'_>, run: &Run) -> String {
    format!(
        "{:?}",
        (
            normalized_text(Some(run.id.as_str())),
            normalized_text(run.start_time_stamp.as_deref()),
            normalized_text(run.default_instrument_configuration_ref.as_deref()),
            normalized_text(run.default_source_file_ref.as_deref()),
            normalized_text(run.sample_ref.as_deref()),
            ctx.effective_param_signatures(
                &run.referenceable_param_group_refs,
                &run.cv_params,
                &run.user_params,
            ),
            source_file_refs_signature(run.source_file_ref_list.as_ref()),
        )
    )
}

fn spectrum_description_params_signature(
    ctx: &SemanticCtx<'_>,
    description: Option<&SpectrumDescription>,
) -> Vec<String> {
    description
        .map(|description| {
            ctx.effective_param_signatures(
                &description.referenceable_param_group_refs,
                &description.cv_params,
                &description.user_params,
            )
        })
        .unwrap_or_default()
}

fn binary_variant_name(binary: &BinaryData) -> &'static str {
    match binary {
        BinaryData::F64(_) => "f64",
        BinaryData::F32(_) => "f32",
        BinaryData::F16(_) => "f16",
        BinaryData::I64(_) => "i64",
        BinaryData::I32(_) => "i32",
        BinaryData::I16(_) => "i16",
    }
}

fn effective_binary_data_array_length(array: &BinaryDataArray) -> Option<usize> {
    array
        .array_length
        .or_else(|| array.binary.as_ref().map(binary_len))
}

fn assert_binary_data_array_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: &BinaryDataArray,
    right: &BinaryDataArray,
    ctx: &str,
) {
    assert_eq!(
        effective_binary_data_array_length(left),
        effective_binary_data_array_length(right),
        "{ctx}: arrayLength mismatch"
    );
    assert_opt_str_eq(
        left.data_processing_ref.as_deref(),
        right.data_processing_ref.as_deref(),
        &format!("{ctx}: dataProcessingRef mismatch"),
    );
    assert_eq!(
        left.numeric_type, right.numeric_type,
        "{ctx}: numeric_type mismatch"
    );
    assert_effective_params_eq(
        left_ctx,
        &left.referenceable_param_group_refs,
        &left.cv_params,
        &left.user_params,
        right_ctx,
        &right.referenceable_param_group_refs,
        &right.cv_params,
        &right.user_params,
        &format!("{ctx}: parameter bundle mismatch"),
    );

    match (left.binary.as_ref(), right.binary.as_ref()) {
        (Some(left_binary), Some(right_binary)) => {
            assert_eq!(
                binary_variant_name(left_binary),
                binary_variant_name(right_binary),
                "{ctx}: binary variant mismatch"
            );
            assert_binary_semantic_eq(left_binary, right_binary, ctx);
        }
        _ => {
            assert!(
                binary_semantically_empty(left.binary.as_ref())
                    && binary_semantically_empty(right.binary.as_ref()),
                "{ctx}: one side has semantic payload while the other is empty"
            );
        }
    }
}

fn assert_binary_data_array_list_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: Option<&BinaryDataArrayList>,
    right: Option<&BinaryDataArrayList>,
    ctx: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_eq!(
                left.binary_data_arrays.len(),
                right.binary_data_arrays.len(),
                "{ctx}: binaryDataArray count mismatch"
            );
            for (index, (left, right)) in left
                .binary_data_arrays
                .iter()
                .zip(&right.binary_data_arrays)
                .enumerate()
            {
                let array_ctx = format!("{ctx} array[{index}] role={}", bda_role(left));
                assert_eq!(
                    bda_role(left),
                    bda_role(right),
                    "{array_ctx}: role mismatch"
                );
                assert_binary_data_array_semantic_eq(left_ctx, right_ctx, left, right, &array_ctx);
            }
        }
        _ => panic!("{ctx}: binaryDataArrayList presence mismatch"),
    }
}

fn assert_scan_window_list_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: Option<&ScanWindowList>,
    right: Option<&ScanWindowList>,
    ctx: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_eq!(
                left.scan_windows.len(),
                right.scan_windows.len(),
                "{ctx}: scanWindow count mismatch"
            );
            for (index, (left_window, right_window)) in left
                .scan_windows
                .iter()
                .zip(&right.scan_windows)
                .enumerate()
            {
                assert_effective_params_eq(
                    left_ctx,
                    &[],
                    &left_window.cv_params,
                    &left_window.user_params,
                    right_ctx,
                    &[],
                    &right_window.cv_params,
                    &right_window.user_params,
                    &format!("{ctx}: scanWindow[{index}] params mismatch"),
                );
            }
        }
        _ => panic!("{ctx}: scanWindowList presence mismatch"),
    }
}

fn assert_scan_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: &Scan,
    right: &Scan,
    ctx: &str,
) {
    assert_opt_str_eq(
        left.instrument_configuration_ref.as_deref(),
        right.instrument_configuration_ref.as_deref(),
        &format!("{ctx}: instrumentConfigurationRef mismatch"),
    );
    assert_opt_str_eq(
        left.external_spectrum_id.as_deref(),
        right.external_spectrum_id.as_deref(),
        &format!("{ctx}: externalSpectrumID mismatch"),
    );
    assert_opt_str_eq(
        left.source_file_ref.as_deref(),
        right.source_file_ref.as_deref(),
        &format!("{ctx}: sourceFileRef mismatch"),
    );
    assert_opt_str_eq(
        left.spectrum_ref.as_deref(),
        right.spectrum_ref.as_deref(),
        &format!("{ctx}: spectrumRef mismatch"),
    );
    assert_effective_params_eq(
        left_ctx,
        &left.referenceable_param_group_refs,
        &left.cv_params,
        &left.user_params,
        right_ctx,
        &right.referenceable_param_group_refs,
        &right.cv_params,
        &right.user_params,
        &format!("{ctx}: parameter bundle mismatch"),
    );
    assert_scan_window_list_semantic_eq(
        left_ctx,
        right_ctx,
        left.scan_window_list.as_ref(),
        right.scan_window_list.as_ref(),
        &format!("{ctx}: scanWindowList"),
    );
}

fn assert_scan_list_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: Option<&ScanList>,
    right: Option<&ScanList>,
    ctx: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_effective_params_eq(
                left_ctx,
                &[],
                &left.cv_params,
                &left.user_params,
                right_ctx,
                &[],
                &right.cv_params,
                &right.user_params,
                &format!("{ctx}: scanList params mismatch"),
            );
            assert_eq!(
                left.scans.len(),
                right.scans.len(),
                "{ctx}: scan count mismatch"
            );
            for (index, (left_scan, right_scan)) in left.scans.iter().zip(&right.scans).enumerate()
            {
                assert_scan_semantic_eq(
                    left_ctx,
                    right_ctx,
                    left_scan,
                    right_scan,
                    &format!("{ctx}: scan[{index}]"),
                );
            }
        }
        _ => panic!("{ctx}: scanList presence mismatch"),
    }
}

fn assert_isolation_window_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: Option<&IsolationWindow>,
    right: Option<&IsolationWindow>,
    ctx: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_effective_params_eq(
                left_ctx,
                &left.referenceable_param_group_refs,
                &left.cv_params,
                &left.user_params,
                right_ctx,
                &right.referenceable_param_group_refs,
                &right.cv_params,
                &right.user_params,
                ctx,
            );
        }
        _ => panic!("{ctx}: isolationWindow presence mismatch"),
    }
}

fn assert_selected_ion_list_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: Option<&SelectedIonList>,
    right: Option<&SelectedIonList>,
    ctx: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_eq!(
                left.selected_ions.len(),
                right.selected_ions.len(),
                "{ctx}: selectedIon count mismatch"
            );
            for (index, (left_ion, right_ion)) in left
                .selected_ions
                .iter()
                .zip(&right.selected_ions)
                .enumerate()
            {
                assert_effective_params_eq(
                    left_ctx,
                    &left_ion.referenceable_param_group_refs,
                    &left_ion.cv_params,
                    &left_ion.user_params,
                    right_ctx,
                    &right_ion.referenceable_param_group_refs,
                    &right_ion.cv_params,
                    &right_ion.user_params,
                    &format!("{ctx}: selectedIon[{index}] params mismatch"),
                );
            }
        }
        _ => panic!("{ctx}: selectedIonList presence mismatch"),
    }
}

fn assert_activation_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: Option<&Activation>,
    right: Option<&Activation>,
    ctx: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_effective_params_eq(
                left_ctx,
                &left.referenceable_param_group_refs,
                &left.cv_params,
                &left.user_params,
                right_ctx,
                &right.referenceable_param_group_refs,
                &right.cv_params,
                &right.user_params,
                ctx,
            );
        }
        _ => panic!("{ctx}: activation presence mismatch"),
    }
}

fn assert_precursor_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: &Precursor,
    right: &Precursor,
    ctx: &str,
) {
    assert_opt_str_eq(
        left.spectrum_ref.as_deref(),
        right.spectrum_ref.as_deref(),
        &format!("{ctx}: spectrumRef mismatch"),
    );
    assert_opt_str_eq(
        left.source_file_ref.as_deref(),
        right.source_file_ref.as_deref(),
        &format!("{ctx}: sourceFileRef mismatch"),
    );
    assert_opt_str_eq(
        left.external_spectrum_id.as_deref(),
        right.external_spectrum_id.as_deref(),
        &format!("{ctx}: externalSpectrumID mismatch"),
    );
    assert_isolation_window_semantic_eq(
        left_ctx,
        right_ctx,
        left.isolation_window.as_ref(),
        right.isolation_window.as_ref(),
        &format!("{ctx}: isolationWindow"),
    );
    assert_selected_ion_list_semantic_eq(
        left_ctx,
        right_ctx,
        left.selected_ion_list.as_ref(),
        right.selected_ion_list.as_ref(),
        &format!("{ctx}: selectedIonList"),
    );
    assert_activation_semantic_eq(
        left_ctx,
        right_ctx,
        left.activation.as_ref(),
        right.activation.as_ref(),
        &format!("{ctx}: activation"),
    );
}

fn assert_precursor_list_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: Option<&PrecursorList>,
    right: Option<&PrecursorList>,
    ctx: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_effective_params_eq(
                left_ctx,
                &[],
                &left.cv_params,
                &left.user_params,
                right_ctx,
                &[],
                &right.cv_params,
                &right.user_params,
                &format!("{ctx}: precursorList params mismatch"),
            );
            assert_eq!(
                left.precursors.len(),
                right.precursors.len(),
                "{ctx}: precursor count mismatch"
            );
            for (index, (left_precursor, right_precursor)) in
                left.precursors.iter().zip(&right.precursors).enumerate()
            {
                assert_precursor_semantic_eq(
                    left_ctx,
                    right_ctx,
                    left_precursor,
                    right_precursor,
                    &format!("{ctx}: precursor[{index}]"),
                );
            }
        }
        _ => panic!("{ctx}: precursorList presence mismatch"),
    }
}

fn assert_product_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: &Product,
    right: &Product,
    ctx: &str,
) {
    assert_opt_str_eq(
        left.spectrum_ref.as_deref(),
        right.spectrum_ref.as_deref(),
        &format!("{ctx}: spectrumRef mismatch"),
    );
    assert_opt_str_eq(
        left.source_file_ref.as_deref(),
        right.source_file_ref.as_deref(),
        &format!("{ctx}: sourceFileRef mismatch"),
    );
    assert_opt_str_eq(
        left.external_spectrum_id.as_deref(),
        right.external_spectrum_id.as_deref(),
        &format!("{ctx}: externalSpectrumID mismatch"),
    );
    assert_isolation_window_semantic_eq(
        left_ctx,
        right_ctx,
        left.isolation_window.as_ref(),
        right.isolation_window.as_ref(),
        &format!("{ctx}: isolationWindow"),
    );
    assert_effective_params_eq(
        left_ctx,
        &[],
        &left.cv_params,
        &left.user_params,
        right_ctx,
        &[],
        &right.cv_params,
        &right.user_params,
        &format!("{ctx}: parameter bundle mismatch"),
    );
}

fn assert_product_list_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: Option<&ProductList>,
    right: Option<&ProductList>,
    ctx: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_effective_params_eq(
                left_ctx,
                &[],
                &left.cv_params,
                &left.user_params,
                right_ctx,
                &[],
                &right.cv_params,
                &right.user_params,
                &format!("{ctx}: productList params mismatch"),
            );
            assert_eq!(
                left.products.len(),
                right.products.len(),
                "{ctx}: product count mismatch"
            );
            for (index, (left_product, right_product)) in
                left.products.iter().zip(&right.products).enumerate()
            {
                assert_product_semantic_eq(
                    left_ctx,
                    right_ctx,
                    left_product,
                    right_product,
                    &format!("{ctx}: product[{index}]"),
                );
            }
        }
        _ => panic!("{ctx}: productList presence mismatch"),
    }
}

fn assert_spectrum_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: &Spectrum,
    right: &Spectrum,
    left_default_data_processing_ref: Option<&str>,
    right_default_data_processing_ref: Option<&str>,
    ctx: &str,
) {
    assert_eq!(left.id, right.id, "{ctx}: spectrum id mismatch");
    assert_eq!(left.index, right.index, "{ctx}: spectrum index mismatch");
    assert_eq!(
        left.scan_number, right.scan_number,
        "{ctx}: scan number mismatch"
    );
    assert_eq!(
        left.default_array_length, right.default_array_length,
        "{ctx}: defaultArrayLength mismatch"
    );
    assert_opt_str_eq(
        left.native_id.as_deref(),
        right.native_id.as_deref(),
        &format!("{ctx}: nativeID mismatch"),
    );
    assert_opt_str_eq(
        effective_data_processing_ref(
            left.data_processing_ref.as_deref(),
            left_default_data_processing_ref,
        ),
        effective_data_processing_ref(
            right.data_processing_ref.as_deref(),
            right_default_data_processing_ref,
        ),
        &format!("{ctx}: dataProcessingRef mismatch"),
    );
    assert_opt_str_eq(
        left.source_file_ref.as_deref(),
        right.source_file_ref.as_deref(),
        &format!("{ctx}: sourceFileRef mismatch"),
    );
    assert_opt_str_eq(
        left.spot_id.as_deref(),
        right.spot_id.as_deref(),
        &format!("{ctx}: spotID mismatch"),
    );
    assert_eq!(left.ms_level, right.ms_level, "{ctx}: msLevel mismatch");

    assert_effective_params_eq(
        left_ctx,
        &left.referenceable_param_group_refs,
        &left.cv_params,
        &left.user_params,
        right_ctx,
        &right.referenceable_param_group_refs,
        &right.cv_params,
        &right.user_params,
        &format!("{ctx}: spectrum parameter bundle mismatch"),
    );
    assert_signature_vec_eq(
        spectrum_description_params_signature(left_ctx, left.spectrum_description.as_ref()),
        spectrum_description_params_signature(right_ctx, right.spectrum_description.as_ref()),
        &format!("{ctx}: spectrumDescription parameter bundle mismatch"),
    );

    assert_scan_list_semantic_eq(
        left_ctx,
        right_ctx,
        scan_list_of_spectrum(left),
        scan_list_of_spectrum(right),
        &format!("{ctx}: scanList"),
    );
    assert_precursor_list_semantic_eq(
        left_ctx,
        right_ctx,
        precursor_list_of_spectrum(left),
        precursor_list_of_spectrum(right),
        &format!("{ctx}: precursorList"),
    );
    assert_product_list_semantic_eq(
        left_ctx,
        right_ctx,
        product_list_of_spectrum(left),
        product_list_of_spectrum(right),
        &format!("{ctx}: productList"),
    );
    assert_binary_data_array_list_semantic_eq(
        left_ctx,
        right_ctx,
        left.binary_data_array_list.as_ref(),
        right.binary_data_array_list.as_ref(),
        &format!("{ctx}: binaryDataArrayList"),
    );
}

fn assert_chromatogram_semantic_eq(
    left_ctx: &SemanticCtx<'_>,
    right_ctx: &SemanticCtx<'_>,
    left: &Chromatogram,
    right: &Chromatogram,
    left_default_data_processing_ref: Option<&str>,
    right_default_data_processing_ref: Option<&str>,
    ctx: &str,
) {
    assert_eq!(left.id, right.id, "{ctx}: chromatogram id mismatch");
    assert_opt_str_eq(
        left.native_id.as_deref(),
        right.native_id.as_deref(),
        &format!("{ctx}: chromatogram nativeID mismatch"),
    );
    assert_eq!(
        left.index, right.index,
        "{ctx}: chromatogram index mismatch"
    );
    assert_eq!(
        left.default_array_length, right.default_array_length,
        "{ctx}: defaultArrayLength mismatch"
    );
    assert_opt_str_eq(
        effective_data_processing_ref(
            left.data_processing_ref.as_deref(),
            left_default_data_processing_ref,
        ),
        effective_data_processing_ref(
            right.data_processing_ref.as_deref(),
            right_default_data_processing_ref,
        ),
        &format!("{ctx}: chromatogram dataProcessingRef mismatch"),
    );
    assert_effective_params_eq(
        left_ctx,
        &left.referenceable_param_group_refs,
        &left.cv_params,
        &left.user_params,
        right_ctx,
        &right.referenceable_param_group_refs,
        &right.cv_params,
        &right.user_params,
        &format!("{ctx}: chromatogram parameter bundle mismatch"),
    );
    match (left.precursor.as_ref(), right.precursor.as_ref()) {
        (None, None) => {}
        (Some(left_precursor), Some(right_precursor)) => assert_precursor_semantic_eq(
            left_ctx,
            right_ctx,
            left_precursor,
            right_precursor,
            &format!("{ctx}: chromatogram precursor"),
        ),
        _ => panic!("{ctx}: chromatogram precursor presence mismatch"),
    }
    match (left.product.as_ref(), right.product.as_ref()) {
        (None, None) => {}
        (Some(left_product), Some(right_product)) => assert_product_semantic_eq(
            left_ctx,
            right_ctx,
            left_product,
            right_product,
            &format!("{ctx}: chromatogram product"),
        ),
        _ => panic!("{ctx}: chromatogram product presence mismatch"),
    }
    assert_binary_data_array_list_semantic_eq(
        left_ctx,
        right_ctx,
        left.binary_data_array_list.as_ref(),
        right.binary_data_array_list.as_ref(),
        &format!("{ctx}: chromatogram binaryDataArrayList"),
    );
}

fn assert_declared_counts_consistent(mzml: &MzML) {
    if let Some(list) = mzml.cv_list.as_ref() {
        assert_optional_count_eq("cvList", list.count, list.cv.len());
    }
    if let Some(list) = mzml.referenceable_param_group_list.as_ref() {
        assert_optional_count_eq(
            "referenceableParamGroupList",
            list.count,
            list.referenceable_param_groups.len(),
        );
    }
    if let Some(file_description) = mzml.file_description.as_ref() {
        assert_optional_count_eq(
            "sourceFileList",
            file_description.source_file_list.count,
            file_description.source_file_list.source_file.len(),
        );
    }
    if let Some(list) = mzml.sample_list.as_ref() {
        assert_optional_count_eq_u32("sampleList", list.count, list.samples.len());
    }
    if let Some(list) = mzml.instrument_list.as_ref() {
        assert_optional_count_eq("instrumentList", list.count, list.instrument.len());
        for (index, instrument) in list.instrument.iter().enumerate() {
            if let Some(component_list) = instrument.component_list.as_ref() {
                let actual = component_list.source.len()
                    + component_list.analyzer.len()
                    + component_list.detector.len();
                assert_optional_count_eq(
                    &format!("instrument[{index}].componentList"),
                    component_list.count,
                    actual,
                );
            }
        }
    }
    if let Some(list) = mzml.software_list.as_ref() {
        assert_optional_count_eq("softwareList", list.count, list.software.len());
    }
    if let Some(list) = mzml.data_processing_list.as_ref() {
        assert_optional_count_eq("dataProcessingList", list.count, list.data_processing.len());
    }
    if let Some(list) = mzml.scan_settings_list.as_ref() {
        assert_optional_count_eq("scanSettingsList", list.count, list.scan_settings.len());
        for (index, scan_settings) in list.scan_settings.iter().enumerate() {
            if let Some(source_file_refs) = scan_settings.source_file_ref_list.as_ref() {
                assert_optional_count_eq(
                    &format!("scanSettings[{index}].sourceFileRefList"),
                    source_file_refs.count,
                    source_file_refs.source_file_refs.len(),
                );
            }
            if let Some(target_list) = scan_settings.target_list.as_ref() {
                assert_optional_count_eq(
                    &format!("scanSettings[{index}].targetList"),
                    target_list.count,
                    target_list.targets.len(),
                );
            }
        }
    }
    if let Some(source_file_refs) = mzml.run.source_file_ref_list.as_ref() {
        assert_optional_count_eq(
            "run.sourceFileRefList",
            source_file_refs.count,
            source_file_refs.source_file_refs.len(),
        );
    }
    if let Some(list) = mzml.run.spectrum_list.as_ref() {
        assert_optional_count_eq("spectrumList", list.count, list.spectra.len());
        for (index, spectrum) in list.spectra.iter().enumerate() {
            if let Some(scan_list) = scan_list_of_spectrum(spectrum) {
                assert_optional_count_eq(
                    &format!("spectrum[{index}].scanList"),
                    scan_list.count,
                    scan_list.scans.len(),
                );
                for (scan_index, scan) in scan_list.scans.iter().enumerate() {
                    if let Some(scan_window_list) = scan.scan_window_list.as_ref() {
                        assert_optional_count_eq(
                            &format!("spectrum[{index}].scan[{scan_index}].scanWindowList"),
                            scan_window_list.count,
                            scan_window_list.scan_windows.len(),
                        );
                    }
                }
            }
            if let Some(precursor_list) = precursor_list_of_spectrum(spectrum) {
                assert_optional_count_eq(
                    &format!("spectrum[{index}].precursorList"),
                    precursor_list.count,
                    precursor_list.precursors.len(),
                );
                for (precursor_index, precursor) in precursor_list.precursors.iter().enumerate() {
                    if let Some(selected_ion_list) = precursor.selected_ion_list.as_ref() {
                        assert_optional_count_eq(
                            &format!(
                                "spectrum[{index}].precursor[{precursor_index}].selectedIonList"
                            ),
                            selected_ion_list.count,
                            selected_ion_list.selected_ions.len(),
                        );
                    }
                }
            }
            if let Some(product_list) = product_list_of_spectrum(spectrum) {
                assert_optional_count_eq(
                    &format!("spectrum[{index}].productList"),
                    product_list.count,
                    product_list.products.len(),
                );
            }
            if let Some(binary_data_array_list) = spectrum.binary_data_array_list.as_ref() {
                assert_optional_count_eq(
                    &format!("spectrum[{index}].binaryDataArrayList"),
                    binary_data_array_list.count,
                    binary_data_array_list.binary_data_arrays.len(),
                );
                assert_binary_data_array_lengths_consistent(
                    &binary_data_array_list.binary_data_arrays,
                    &format!("spectrum[{index}]"),
                );
            }
        }
    }
    if let Some(list) = mzml.run.chromatogram_list.as_ref() {
        assert_optional_count_eq("chromatogramList", list.count, list.chromatograms.len());
        for (index, chromatogram) in list.chromatograms.iter().enumerate() {
            if let Some(binary_data_array_list) = chromatogram.binary_data_array_list.as_ref() {
                assert_optional_count_eq(
                    &format!("chromatogram[{index}].binaryDataArrayList"),
                    binary_data_array_list.count,
                    binary_data_array_list.binary_data_arrays.len(),
                );
                assert_binary_data_array_lengths_consistent(
                    &binary_data_array_list.binary_data_arrays,
                    &format!("chromatogram[{index}]"),
                );
            }
        }
    }
}

fn assert_mzml_semantic_eq(left: &MzML, right: &MzML) {
    assert_all_refs_resolved(left);
    assert_all_refs_resolved(right);

    let left_ctx = SemanticCtx::new(left);
    let right_ctx = SemanticCtx::new(right);

    assert_signature_vec_eq(
        left.cv_list
            .as_ref()
            .map(|list| {
                sorted_signatures(list.cv.iter().map(|entry| {
                    format!(
                        "{:?}",
                        (
                            normalized_text(Some(entry.id.as_str())),
                            normalized_text(entry.full_name.as_deref()),
                            normalized_text(entry.version.as_deref()),
                            normalized_text(entry.uri.as_deref()),
                        )
                    )
                }))
            })
            .unwrap_or_default(),
        right
            .cv_list
            .as_ref()
            .map(|list| {
                sorted_signatures(list.cv.iter().map(|entry| {
                    format!(
                        "{:?}",
                        (
                            normalized_text(Some(entry.id.as_str())),
                            normalized_text(entry.full_name.as_deref()),
                            normalized_text(entry.version.as_deref()),
                            normalized_text(entry.uri.as_deref()),
                        )
                    )
                }))
            })
            .unwrap_or_default(),
        "cvList mismatch",
    );

    match (
        left.file_description.as_ref(),
        right.file_description.as_ref(),
    ) {
        (None, None) => {}
        (Some(left_file_description), Some(right_file_description)) => {
            assert_effective_params_eq(
                &left_ctx,
                &left_file_description
                    .file_content
                    .referenceable_param_group_refs,
                &left_file_description.file_content.cv_params,
                &left_file_description.file_content.user_params,
                &right_ctx,
                &right_file_description
                    .file_content
                    .referenceable_param_group_refs,
                &right_file_description.file_content.cv_params,
                &right_file_description.file_content.user_params,
                "fileDescription.fileContent semantic mismatch",
            );
            assert_signature_vec_eq(
                sorted_signatures(
                    left_file_description
                        .source_file_list
                        .source_file
                        .iter()
                        .map(|item| source_file_signature(&left_ctx, item)),
                ),
                sorted_signatures(
                    right_file_description
                        .source_file_list
                        .source_file
                        .iter()
                        .map(|item| source_file_signature(&right_ctx, item)),
                ),
                "fileDescription.sourceFileList mismatch",
            );
            assert_signature_vec_eq(
                sorted_signatures(
                    left_file_description
                        .contacts
                        .iter()
                        .map(|item| contact_signature(&left_ctx, item)),
                ),
                sorted_signatures(
                    right_file_description
                        .contacts
                        .iter()
                        .map(|item| contact_signature(&right_ctx, item)),
                ),
                "fileDescription.contacts mismatch",
            );
        }
        _ => panic!("fileDescription presence mismatch"),
    }

    assert_signature_vec_eq(
        left.sample_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.samples
                        .iter()
                        .map(|item| sample_signature(&left_ctx, item)),
                )
            })
            .unwrap_or_default(),
        right
            .sample_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.samples
                        .iter()
                        .map(|item| sample_signature(&right_ctx, item)),
                )
            })
            .unwrap_or_default(),
        "sampleList mismatch",
    );
    assert_signature_vec_eq(
        left.instrument_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.instrument
                        .iter()
                        .map(|item| instrument_signature(&left_ctx, item)),
                )
            })
            .unwrap_or_default(),
        right
            .instrument_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.instrument
                        .iter()
                        .map(|item| instrument_signature(&right_ctx, item)),
                )
            })
            .unwrap_or_default(),
        "instrumentList mismatch",
    );
    assert_signature_vec_eq(
        left.software_list
            .as_ref()
            .map(|list| sorted_signatures(list.software.iter().map(software_signature)))
            .unwrap_or_default(),
        right
            .software_list
            .as_ref()
            .map(|list| sorted_signatures(list.software.iter().map(software_signature)))
            .unwrap_or_default(),
        "softwareList mismatch",
    );
    assert_signature_vec_eq(
        left.data_processing_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.data_processing
                        .iter()
                        .map(|item| data_processing_signature(&left_ctx, item)),
                )
            })
            .unwrap_or_default(),
        right
            .data_processing_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.data_processing
                        .iter()
                        .map(|item| data_processing_signature(&right_ctx, item)),
                )
            })
            .unwrap_or_default(),
        "dataProcessingList mismatch",
    );
    assert_signature_vec_eq(
        left.scan_settings_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.scan_settings
                        .iter()
                        .map(|item| scan_settings_signature(&left_ctx, item)),
                )
            })
            .unwrap_or_default(),
        right
            .scan_settings_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.scan_settings
                        .iter()
                        .map(|item| scan_settings_signature(&right_ctx, item)),
                )
            })
            .unwrap_or_default(),
        "scanSettingsList mismatch",
    );

    assert_eq!(
        run_signature(&left_ctx, &left.run),
        run_signature(&right_ctx, &right.run),
        "run metadata mismatch"
    );

    match (
        left.run.spectrum_list.as_ref(),
        right.run.spectrum_list.as_ref(),
    ) {
        (Some(left_spectrum_list), Some(right_spectrum_list)) => {
            assert_eq!(
                left_spectrum_list.spectra.len(),
                right_spectrum_list.spectra.len(),
                "spectrum count mismatch"
            );
            for (index, (left_spectrum, right_spectrum)) in left_spectrum_list
                .spectra
                .iter()
                .zip(&right_spectrum_list.spectra)
                .enumerate()
            {
                assert_spectrum_semantic_eq(
                    &left_ctx,
                    &right_ctx,
                    left_spectrum,
                    right_spectrum,
                    left_spectrum_list.default_data_processing_ref.as_deref(),
                    right_spectrum_list.default_data_processing_ref.as_deref(),
                    &format!("spectrum[{index}]"),
                );
            }
        }
        (None, None) => {}
        _ => panic!("spectrumList presence mismatch"),
    }

    match (
        left.run.chromatogram_list.as_ref(),
        right.run.chromatogram_list.as_ref(),
    ) {
        (Some(left_chromatogram_list), Some(right_chromatogram_list)) => {
            assert_eq!(
                left_chromatogram_list.chromatograms.len(),
                right_chromatogram_list.chromatograms.len(),
                "chromatogram count mismatch"
            );
            for (index, (left_chromatogram, right_chromatogram)) in left_chromatogram_list
                .chromatograms
                .iter()
                .zip(&right_chromatogram_list.chromatograms)
                .enumerate()
            {
                assert_chromatogram_semantic_eq(
                    &left_ctx,
                    &right_ctx,
                    left_chromatogram,
                    right_chromatogram,
                    left_chromatogram_list
                        .default_data_processing_ref
                        .as_deref(),
                    right_chromatogram_list
                        .default_data_processing_ref
                        .as_deref(),
                    &format!("chromatogram[{index}]"),
                );
            }
        }
        (None, None) => {}
        _ => panic!("chromatogramList presence mismatch"),
    }
}

fn assert_mzml_structural_eq(left: &MzML, right: &MzML) {
    assert_eq!(left.run.id, right.run.id, "run id mismatch");
    assert_eq!(
        spectra(left).len(),
        spectra(right).len(),
        "spectrum count mismatch"
    );
    assert_eq!(
        chromatograms(left).len(),
        chromatograms(right).len(),
        "chromatogram count mismatch"
    );

    let left_ids: Vec<_> = spectra(left).iter().map(|s| s.id.as_str()).collect();
    let right_ids: Vec<_> = spectra(right).iter().map(|s| s.id.as_str()).collect();
    assert_eq!(left_ids, right_ids, "spectrum ids mismatch");

    let left_chrom_ids: Vec<_> = chromatograms(left).iter().map(|c| c.id.as_str()).collect();
    let right_chrom_ids: Vec<_> = chromatograms(right).iter().map(|c| c.id.as_str()).collect();
    assert_eq!(left_chrom_ids, right_chrom_ids, "chromatogram ids mismatch");
}

fn assert_referenceable_param_group_refs_resolved(
    refs: &[ReferenceableParamGroupRef],
    ref_group_ids: &BTreeSet<String>,
    ctx: &str,
) {
    for group_ref in refs {
        assert!(
            ref_group_ids.contains(group_ref.r#ref.as_str()),
            "{ctx} unresolved referenceableParamGroupRef: {}",
            group_ref.r#ref
        );
    }
}

fn assert_binary_data_array_lengths_consistent(arrays: &[BinaryDataArray], ctx: &str) {
    let mut canonical_len = None;
    for (index, array) in arrays.iter().enumerate() {
        let array_ctx = format!("{ctx} binaryDataArray[{index}]");
        if let (Some(binary), Some(array_length)) = (array.binary.as_ref(), array.array_length) {
            assert_eq!(
                binary_len(binary),
                array_length,
                "{array_ctx}: arrayLength does not match payload length"
            );
        }
        if let Some(binary) = array.binary.as_ref() {
            let len = binary_len(binary);
            if len > 0 {
                if let Some(existing) = canonical_len {
                    assert_eq!(
                        len, existing,
                        "{array_ctx}: payload length mismatch across arrays"
                    );
                } else {
                    canonical_len = Some(len);
                }
            }
        }
    }
}

fn assert_all_refs_resolved(mzml: &MzML) {
    let source_file_ids = top_level_source_file_ids(mzml);
    let software_ids = top_level_software_ids(mzml);
    let dp_ids = top_level_dp_ids(mzml);
    let instrument_ids = top_level_instrument_ids(mzml);
    let sample_ids = top_level_sample_ids(mzml);
    let ref_group_ids = mzml
        .referenceable_param_group_list
        .as_ref()
        .map(|list| {
            set_of_ids(&list.referenceable_param_groups, |group| {
                Some(group.id.as_str())
            })
        })
        .unwrap_or_default();
    let scan_settings_ids = mzml
        .scan_settings_list
        .as_ref()
        .map(|list| set_of_ids(&list.scan_settings, |item| item.id.as_deref()))
        .unwrap_or_default();
    let spectrum_ids = set_of_ids(spectra(mzml), |s| Some(s.id.as_str()));

    assert_referenceable_param_group_refs_resolved(
        &mzml.run.referenceable_param_group_refs,
        &ref_group_ids,
        "run",
    );

    if let Some(file_description) = mzml.file_description.as_ref() {
        assert_referenceable_param_group_refs_resolved(
            &file_description.file_content.referenceable_param_group_refs,
            &ref_group_ids,
            "fileDescription.fileContent",
        );
        for (index, source_file) in file_description
            .source_file_list
            .source_file
            .iter()
            .enumerate()
        {
            assert_referenceable_param_group_refs_resolved(
                &source_file.referenceable_param_group_ref,
                &ref_group_ids,
                &format!("sourceFile[{index}]"),
            );
        }
        for (index, contact) in file_description.contacts.iter().enumerate() {
            assert_referenceable_param_group_refs_resolved(
                &contact.referenceable_param_group_refs,
                &ref_group_ids,
                &format!("contact[{index}]"),
            );
        }
    }

    if let Some(sample_list) = mzml.sample_list.as_ref() {
        for (index, sample) in sample_list.samples.iter().enumerate() {
            if let Some(group_ref) = sample.referenceable_param_group_ref.as_ref() {
                assert_referenceable_param_group_refs_resolved(
                    std::slice::from_ref(group_ref),
                    &ref_group_ids,
                    &format!("sample[{index}]"),
                );
            }
        }
    }

    if let Some(r) = mzml.run.default_source_file_ref.as_deref() {
        assert!(
            source_file_ids.contains(r),
            "run.defaultSourceFileRef unresolved: {r}"
        );
    }
    if let Some(r) = mzml.run.default_instrument_configuration_ref.as_deref() {
        assert!(
            instrument_ids.contains(r),
            "run.defaultInstrumentConfigurationRef unresolved: {r}"
        );
    }
    if let Some(r) = mzml.run.sample_ref.as_deref() {
        assert!(sample_ids.contains(r), "run.sampleRef unresolved: {r}");
    }

    if let Some(sfrl) = mzml.run.source_file_ref_list.as_ref() {
        for sr in &sfrl.source_file_refs {
            assert!(
                source_file_ids.contains(sr.r#ref.as_str()),
                "run.sourceFileRefList unresolved ref: {}",
                sr.r#ref
            );
        }
    }

    if let Some(ssl) = mzml.scan_settings_list.as_ref() {
        for (index, ss) in ssl.scan_settings.iter().enumerate() {
            assert_referenceable_param_group_refs_resolved(
                &ss.referenceable_param_group_refs,
                &ref_group_ids,
                &format!("scanSettings[{index}]"),
            );
            if let Some(sfrl) = ss.source_file_ref_list.as_ref() {
                for sr in &sfrl.source_file_refs {
                    assert!(
                        source_file_ids.contains(sr.r#ref.as_str()),
                        "scanSettings sourceFileRef unresolved: {}",
                        sr.r#ref
                    );
                }
            }
            if let Some(icr) = ss.instrument_configuration_ref.as_deref() {
                assert!(
                    instrument_ids.contains(icr),
                    "scanSettings instrumentConfigurationRef unresolved: {icr}"
                );
            }
            if let Some(target_list) = ss.target_list.as_ref() {
                for (target_index, target) in target_list.targets.iter().enumerate() {
                    assert_referenceable_param_group_refs_resolved(
                        &target.referenceable_param_group_refs,
                        &ref_group_ids,
                        &format!("scanSettings[{index}].target[{target_index}]"),
                    );
                }
            }
        }
    }

    if let Some(il) = mzml.instrument_list.as_ref() {
        for (index, ic) in il.instrument.iter().enumerate() {
            assert_referenceable_param_group_refs_resolved(
                &ic.referenceable_param_group_ref,
                &ref_group_ids,
                &format!("instrument[{index}]"),
            );
            if let Some(sr) = ic.software_ref.as_ref() {
                assert!(
                    software_ids.contains(sr.r#ref.as_str()),
                    "instrument softwareRef unresolved: {}",
                    sr.r#ref
                );
            }
            if let Some(scan_settings_ref) = ic.scan_settings_ref.as_ref() {
                assert!(
                    scan_settings_ids.contains(scan_settings_ref.r#ref.as_str()),
                    "instrument scanSettingsRef unresolved: {}",
                    scan_settings_ref.r#ref
                );
            }
            if let Some(component_list) = ic.component_list.as_ref() {
                for (component_index, component) in component_list.source.iter().enumerate() {
                    assert_referenceable_param_group_refs_resolved(
                        &component.referenceable_param_group_ref,
                        &ref_group_ids,
                        &format!("instrument[{index}].source[{component_index}]"),
                    );
                }
                for (component_index, component) in component_list.analyzer.iter().enumerate() {
                    assert_referenceable_param_group_refs_resolved(
                        &component.referenceable_param_group_ref,
                        &ref_group_ids,
                        &format!("instrument[{index}].analyzer[{component_index}]"),
                    );
                }
                for (component_index, component) in component_list.detector.iter().enumerate() {
                    assert_referenceable_param_group_refs_resolved(
                        &component.referenceable_param_group_ref,
                        &ref_group_ids,
                        &format!("instrument[{index}].detector[{component_index}]"),
                    );
                }
            }
        }
    }

    if let Some(dpl) = mzml.data_processing_list.as_ref() {
        for (index, dp) in dpl.data_processing.iter().enumerate() {
            if let Some(sr) = dp.software_ref.as_deref() {
                assert!(
                    software_ids.contains(sr),
                    "dataProcessing softwareRef unresolved: {sr}"
                );
            }
            for (method_index, pm) in dp.processing_method.iter().enumerate() {
                assert_referenceable_param_group_refs_resolved(
                    &pm.referenceable_param_group_ref,
                    &ref_group_ids,
                    &format!("dataProcessing[{index}].processingMethod[{method_index}]"),
                );
                if let Some(sr) = pm.software_ref.as_deref() {
                    assert!(
                        software_ids.contains(sr),
                        "processingMethod softwareRef unresolved: {sr}"
                    );
                }
            }
        }
    }

    let run_default_dp = mzml
        .run
        .spectrum_list
        .as_ref()
        .and_then(|sl| sl.default_data_processing_ref.as_deref())
        .or_else(|| {
            mzml.run
                .chromatogram_list
                .as_ref()
                .and_then(|cl| cl.default_data_processing_ref.as_deref())
        });

    for s in spectra(mzml) {
        assert_referenceable_param_group_refs_resolved(
            &s.referenceable_param_group_refs,
            &ref_group_ids,
            &format!("spectrum {}", s.id),
        );
        if let Some(sr) = s.source_file_ref.as_deref() {
            assert!(
                source_file_ids.contains(sr),
                "spectrum sourceFileRef unresolved: {sr}"
            );
        }
        if let Some(dpr) = s.data_processing_ref.as_deref().or(run_default_dp) {
            assert!(
                dp_ids.contains(dpr),
                "spectrum dataProcessingRef unresolved: {dpr}"
            );
        }

        let scan_list = if let Some(sd) = s.spectrum_description.as_ref() {
            assert_referenceable_param_group_refs_resolved(
                &sd.referenceable_param_group_refs,
                &ref_group_ids,
                &format!("spectrumDescription {}", s.id),
            );
            sd.scan_list.as_ref()
        } else {
            s.scan_list.as_ref()
        };

        if let Some(scan_list) = scan_list {
            for scan in &scan_list.scans {
                assert_referenceable_param_group_refs_resolved(
                    &scan.referenceable_param_group_refs,
                    &ref_group_ids,
                    &format!("scan in spectrum {}", s.id),
                );
                if let Some(icr) = scan.instrument_configuration_ref.as_deref() {
                    assert!(
                        instrument_ids.contains(icr),
                        "scan instrumentConfigurationRef unresolved: {icr}"
                    );
                }
                if let Some(sfr) = scan.source_file_ref.as_deref() {
                    assert!(
                        source_file_ids.contains(sfr),
                        "scan sourceFileRef unresolved: {sfr}"
                    );
                }
            }
        }

        let precursor_list = if let Some(sd) = s.spectrum_description.as_ref() {
            sd.precursor_list.as_ref()
        } else {
            s.precursor_list.as_ref()
        };

        if let Some(precursor_list) = precursor_list {
            for p in &precursor_list.precursors {
                if let Some(sr) = p.spectrum_ref.as_deref() {
                    assert!(
                        spectrum_ids.contains(sr),
                        "precursor spectrumRef unresolved: {sr}"
                    );
                }
                if let Some(sfr) = p.source_file_ref.as_deref() {
                    assert!(
                        source_file_ids.contains(sfr),
                        "precursor sourceFileRef unresolved: {sfr}"
                    );
                }
                if let Some(isolation_window) = p.isolation_window.as_ref() {
                    assert_referenceable_param_group_refs_resolved(
                        &isolation_window.referenceable_param_group_refs,
                        &ref_group_ids,
                        &format!("precursor isolationWindow in spectrum {}", s.id),
                    );
                }
                if let Some(selected_ion_list) = p.selected_ion_list.as_ref() {
                    for selected_ion in &selected_ion_list.selected_ions {
                        assert_referenceable_param_group_refs_resolved(
                            &selected_ion.referenceable_param_group_refs,
                            &ref_group_ids,
                            &format!("selectedIon in spectrum {}", s.id),
                        );
                    }
                }
                if let Some(activation) = p.activation.as_ref() {
                    assert_referenceable_param_group_refs_resolved(
                        &activation.referenceable_param_group_refs,
                        &ref_group_ids,
                        &format!("activation in spectrum {}", s.id),
                    );
                }
            }
        }

        if let Some(product_list) = product_list_of_spectrum(s) {
            for product in &product_list.products {
                if let Some(sr) = product.spectrum_ref.as_deref() {
                    assert!(
                        spectrum_ids.contains(sr),
                        "product spectrumRef unresolved: {sr}"
                    );
                }
                if let Some(sfr) = product.source_file_ref.as_deref() {
                    assert!(
                        source_file_ids.contains(sfr),
                        "product sourceFileRef unresolved: {sfr}"
                    );
                }
                if let Some(isolation_window) = product.isolation_window.as_ref() {
                    assert_referenceable_param_group_refs_resolved(
                        &isolation_window.referenceable_param_group_refs,
                        &ref_group_ids,
                        &format!("product isolationWindow in spectrum {}", s.id),
                    );
                }
            }
        }

        if let Some(binary_data_array_list) = s.binary_data_array_list.as_ref() {
            for array in &binary_data_array_list.binary_data_arrays {
                assert_referenceable_param_group_refs_resolved(
                    &array.referenceable_param_group_refs,
                    &ref_group_ids,
                    &format!("binaryDataArray in spectrum {}", s.id),
                );
                if let Some(data_processing_ref) = array.data_processing_ref.as_deref() {
                    assert!(
                        dp_ids.contains(data_processing_ref),
                        "binaryDataArray dataProcessingRef unresolved: {data_processing_ref}"
                    );
                }
            }
        }
    }

    for c in chromatograms(mzml) {
        assert_referenceable_param_group_refs_resolved(
            &c.referenceable_param_group_refs,
            &ref_group_ids,
            &format!("chromatogram {}", c.id),
        );
        if let Some(dpr) = c.data_processing_ref.as_deref().or(run_default_dp) {
            assert!(
                dp_ids.contains(dpr),
                "chromatogram dataProcessingRef unresolved: {dpr}"
            );
        }

        if let Some(p) = c.precursor.as_ref() {
            if let Some(sr) = p.spectrum_ref.as_deref() {
                assert!(
                    spectrum_ids.contains(sr),
                    "chrom precursor spectrumRef unresolved: {sr}"
                );
            }
            if let Some(sfr) = p.source_file_ref.as_deref() {
                assert!(
                    source_file_ids.contains(sfr),
                    "chrom precursor sourceFileRef unresolved: {sfr}"
                );
            }
            if let Some(isolation_window) = p.isolation_window.as_ref() {
                assert_referenceable_param_group_refs_resolved(
                    &isolation_window.referenceable_param_group_refs,
                    &ref_group_ids,
                    &format!("chrom precursor isolationWindow {}", c.id),
                );
            }
        }

        if let Some(p) = c.product.as_ref() {
            if let Some(sr) = p.spectrum_ref.as_deref() {
                assert!(
                    spectrum_ids.contains(sr),
                    "chrom product spectrumRef unresolved: {sr}"
                );
            }
            if let Some(sfr) = p.source_file_ref.as_deref() {
                assert!(
                    source_file_ids.contains(sfr),
                    "chrom product sourceFileRef unresolved: {sfr}"
                );
            }
            if let Some(isolation_window) = p.isolation_window.as_ref() {
                assert_referenceable_param_group_refs_resolved(
                    &isolation_window.referenceable_param_group_refs,
                    &ref_group_ids,
                    &format!("chrom product isolationWindow {}", c.id),
                );
            }
        }

        if let Some(binary_data_array_list) = c.binary_data_array_list.as_ref() {
            for array in &binary_data_array_list.binary_data_arrays {
                assert_referenceable_param_group_refs_resolved(
                    &array.referenceable_param_group_refs,
                    &ref_group_ids,
                    &format!("binaryDataArray in chromatogram {}", c.id),
                );
                if let Some(data_processing_ref) = array.data_processing_ref.as_deref() {
                    assert!(
                        dp_ids.contains(data_processing_ref),
                        "chromatogram binaryDataArray dataProcessingRef unresolved: {data_processing_ref}"
                    );
                }
            }
        }
    }
}

fn spectrum_by_id<'a>(mzml: &'a MzML, id: &str) -> &'a Spectrum {
    spectra(mzml)
        .iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("spectrum not found: {id}"))
}

fn chromatogram_by_id<'a>(mzml: &'a MzML, id: &str) -> &'a Chromatogram {
    chromatograms(mzml)
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("chromatogram not found: {id}"))
}

fn find_array_by_accession<'a>(
    arrays: &'a [BinaryDataArray],
    accession: &str,
) -> &'a BinaryDataArray {
    arrays
        .iter()
        .find(|a| cv_has_accession(&a.cv_params, accession))
        .unwrap_or_else(|| panic!("binaryDataArray with accession {accession} not found"))
}

fn first_f64_from_binary(binary: &BinaryData) -> Option<f64> {
    match binary {
        BinaryData::F64(v) => v.first().copied(),
        BinaryData::F32(v) => v.first().map(|x| *x as f64),
        BinaryData::F16(v) => v.first().map(|x| *x as f64),
        BinaryData::I64(v) => v.first().map(|x| *x as f64),
        BinaryData::I32(v) => v.first().map(|x| *x as f64),
        BinaryData::I16(v) => v.first().map(|x| *x as f64),
    }
}

fn binary_to_f64_vec(binary: &BinaryData) -> Vec<f64> {
    match binary {
        BinaryData::F64(v) => v.clone(),
        BinaryData::F32(v) => v.iter().map(|x| *x as f64).collect(),
        BinaryData::F16(v) => v.iter().map(|x| *x as f64).collect(),
        BinaryData::I64(v) => v.iter().map(|x| *x as f64).collect(),
        BinaryData::I32(v) => v.iter().map(|x| *x as f64).collect(),
        BinaryData::I16(v) => v.iter().map(|x| *x as f64).collect(),
    }
}

fn cv_param_by_accession<'a>(cv_params: &'a [CvParam], accession: &str) -> Option<&'a CvParam> {
    cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some(accession))
}

fn cv_value_f64(cv_params: &[CvParam], accession: &str) -> Option<f64> {
    cv_param_by_accession(cv_params, accession)
        .and_then(|p| p.value.as_deref())
        .and_then(|v| v.parse::<f64>().ok())
}

fn parse_scan_number_from_id(id: &str) -> Option<u32> {
    id.split_whitespace()
        .find_map(|tok| tok.strip_prefix("scan="))
        .and_then(|v| v.parse::<u32>().ok())
}

fn scan_list_of_spectrum(s: &Spectrum) -> Option<&ScanList> {
    if let Some(sd) = s.spectrum_description.as_ref() {
        if sd.scan_list.is_some() {
            return sd.scan_list.as_ref();
        }
    }
    s.scan_list.as_ref()
}

fn scan_list_of_spectrum_mut(s: &mut Spectrum) -> Option<&mut ScanList> {
    if let Some(sd) = s.spectrum_description.as_mut() {
        if sd.scan_list.is_some() {
            return sd.scan_list.as_mut();
        }
    }
    s.scan_list.as_mut()
}

fn precursor_list_of_spectrum(s: &Spectrum) -> Option<&PrecursorList> {
    if let Some(sd) = s.spectrum_description.as_ref() {
        if sd.precursor_list.is_some() {
            return sd.precursor_list.as_ref();
        }
    }
    s.precursor_list.as_ref()
}

fn product_list_of_spectrum(s: &Spectrum) -> Option<&ProductList> {
    if let Some(sd) = s.spectrum_description.as_ref() {
        if sd.product_list.is_some() {
            return sd.product_list.as_ref();
        }
    }
    s.product_list.as_ref()
}

fn first_scan(s: &Spectrum) -> &Scan {
    scan_list_of_spectrum(s)
        .and_then(|sl| sl.scans.first())
        .expect("first scan must exist")
}

fn first_scan_mut(s: &mut Spectrum) -> &mut Scan {
    scan_list_of_spectrum_mut(s)
        .and_then(|sl| sl.scans.first_mut())
        .expect("first scan must exist")
}

fn ensure_first_product_mut(s: &mut Spectrum) -> &mut Product {
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

fn ensure_referenceable_param_group(mzml: &mut MzML, id: &str) {
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

fn scan_start_time_seconds(s: &Spectrum) -> Option<f64> {
    let scan = scan_list_of_spectrum(s)?.scans.first()?;
    let p = cv_param_by_accession(&scan.cv_params, "MS:1000016")?;
    let value = p.value.as_deref()?.parse::<f64>().ok()?;
    match p.unit_accession.as_deref() {
        Some("UO:0000031") => Some(value * 60.0), // minute -> second
        _ => Some(value),                         // already seconds or unitless fallback
    }
}

fn id_name_value_pairs(id: &str) -> Vec<(&str, &str)> {
    id.split_whitespace()
        .filter_map(|tok| tok.split_once('='))
        .collect()
}

fn find_name_value_indices(mzml: &MzML, key: &str, value: &str) -> Vec<usize> {
    spectra(mzml)
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let has_pair = id_name_value_pairs(&s.id)
                .into_iter()
                .any(|(k, v)| k == key && v == value);
            if has_pair {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

fn find_spot_id_indices(mzml: &MzML, spot_id: &str) -> Vec<usize> {
    spectra(mzml)
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s.spot_id.as_deref() {
            Some(id) if id == spot_id => Some(i),
            _ => None,
        })
        .collect()
}

fn first_array_values_by_accession(s: &Spectrum, accession: &str) -> Vec<f64> {
    let bda = find_array_by_accession(spectrum_arrays(s), accession);
    let bin = bda.binary.as_ref().expect("binary payload present");
    binary_to_f64_vec(bin)
}

fn first_chrom_array_values_by_accession(c: &Chromatogram, accession: &str) -> Vec<f64> {
    let bda = find_array_by_accession(chromatogram_arrays(c), accession);
    let bin = bda.binary.as_ref().expect("binary payload present");
    binary_to_f64_vec(bin)
}

fn canonical_hash_index(mzml: &MzML) -> BTreeMap<String, u64> {
    let mut idx = BTreeMap::new();
    idx.insert("run/id".to_string(), fnv64_str(mzml.run.id.as_str()));
    idx.insert("count/spectra".to_string(), spectra(mzml).len() as u64);
    idx.insert(
        "count/chromatograms".to_string(),
        chromatograms(mzml).len() as u64,
    );

    for (i, sp) in spectra(mzml).iter().enumerate() {
        idx.insert(format!("spectrum/{i}/id"), fnv64_str(sp.id.as_str()));
        idx.insert(
            format!("spectrum/{i}/ms_level"),
            sp.ms_level.unwrap_or(0) as u64,
        );
        idx.insert(
            format!("spectrum/{i}/default_array_length"),
            sp.default_array_length.unwrap_or(0) as u64,
        );
        idx.insert(
            format!("spectrum/{i}/scan_count"),
            spectrum_scan_count(sp) as u64,
        );
        idx.insert(
            format!("spectrum/{i}/precursor_count"),
            spectrum_precursor_count(sp) as u64,
        );
        idx.insert(
            format!("spectrum/{i}/product_count"),
            spectrum_product_count(sp) as u64,
        );

        for (j, bda) in spectrum_arrays(sp).iter().enumerate() {
            let role = bda_role(bda);
            idx.insert(format!("spectrum/{i}/array/{j}/role"), fnv64_str(role));
            idx.insert(
                format!("spectrum/{i}/array/{j}/len"),
                bda.array_length.unwrap_or(0) as u64,
            );
            idx.insert(
                format!("spectrum/{i}/array/{j}/payload"),
                bda.binary.as_ref().map(hash_binary_payload).unwrap_or(0),
            );
        }
    }

    for (i, ch) in chromatograms(mzml).iter().enumerate() {
        idx.insert(format!("chromatogram/{i}/id"), fnv64_str(ch.id.as_str()));
        idx.insert(
            format!("chromatogram/{i}/default_array_length"),
            ch.default_array_length.unwrap_or(0) as u64,
        );
        for (j, bda) in chromatogram_arrays(ch).iter().enumerate() {
            let role = bda_role(bda);
            idx.insert(format!("chromatogram/{i}/array/{j}/role"), fnv64_str(role));
            idx.insert(
                format!("chromatogram/{i}/array/{j}/len"),
                bda.array_length.unwrap_or(0) as u64,
            );
            idx.insert(
                format!("chromatogram/{i}/array/{j}/payload"),
                bda.binary.as_ref().map(hash_binary_payload).unwrap_or(0),
            );
        }
    }

    idx
}

fn canonical_diff_paths(left: &MzML, right: &MzML) -> Vec<String> {
    let l = canonical_hash_index(left);
    let r = canonical_hash_index(right);
    let mut keys: BTreeSet<String> = l.keys().cloned().collect();
    keys.extend(r.keys().cloned());

    let mut out = Vec::new();
    for k in keys {
        match (l.get(&k), r.get(&k)) {
            (Some(a), Some(b)) if a != b => out.push(format!("{k}: {a:016x} != {b:016x}")),
            (Some(a), None) => out.push(format!("{k}: {a:016x} != <missing>")),
            (None, Some(b)) => out.push(format!("{k}: <missing> != {b:016x}")),
            _ => {}
        }
    }
    out
}

fn semantic_fingerprint(mzml: &MzML) -> String {
    let idx = canonical_hash_index(mzml);
    let mut state = FNV64_OFFSET;
    for (path, h) in idx {
        state = fnv64_update(state, path.as_bytes());
        state = fnv64_update(state, &h.to_le_bytes());
    }
    format!("{state:016x}")
}

static TINY_PWIZ_10_CACHE: OnceLock<MzML> = OnceLock::new();
static TINY_PWIZ_11_CACHE: OnceLock<MzML> = OnceLock::new();
static TINY_PWIZ_111_CACHE: OnceLock<MzML> = OnceLock::new();
static TINY2_PWIZ_10_CACHE: OnceLock<MzML> = OnceLock::new();
static SMALL_PWIZ_10_CACHE: OnceLock<MzML> = OnceLock::new();
static SMALL_PWIZ_11_CACHE: OnceLock<MzML> = OnceLock::new();
static SMALL_ZLIB_PWIZ_11_CACHE: OnceLock<MzML> = OnceLock::new();
static SMALL_MIAPE_PWIZ_11_CACHE: OnceLock<MzML> = OnceLock::new();
static TEST_MZML_CACHE: OnceLock<MzML> = OnceLock::new();

fn tiny_pwiz_10() -> &'static MzML {
    TINY_PWIZ_10_CACHE.get_or_init(|| parse_rel("pwiz/example_data/tiny.pwiz.1.0.mzML", false))
}

fn tiny_pwiz_11() -> &'static MzML {
    TINY_PWIZ_11_CACHE.get_or_init(|| parse_rel("pwiz/example_data/tiny.pwiz.1.1.mzML", false))
}

fn tiny_pwiz_111() -> &'static MzML {
    TINY_PWIZ_111_CACHE.get_or_init(|| parse_rel("pwiz/example_data/tiny.pwiz.1.1.1.mzML", false))
}

fn tiny2_pwiz_10() -> &'static MzML {
    TINY2_PWIZ_10_CACHE.get_or_init(|| parse_rel("pwiz/example_data/tiny2.pwiz.1.0.mzML", false))
}

fn small_pwiz_10() -> &'static MzML {
    SMALL_PWIZ_10_CACHE.get_or_init(|| parse_rel("pwiz/example_data/small.pwiz.1.0.mzML", false))
}

fn small_pwiz_11() -> &'static MzML {
    SMALL_PWIZ_11_CACHE.get_or_init(|| parse_rel("pwiz/example_data/small.pwiz.1.1.mzML", false))
}

fn small_zlib_pwiz_11() -> &'static MzML {
    SMALL_ZLIB_PWIZ_11_CACHE
        .get_or_init(|| parse_rel("pwiz/example_data/small_zlib.pwiz.1.1.mzML", false))
}

fn small_miape_pwiz_11() -> &'static MzML {
    SMALL_MIAPE_PWIZ_11_CACHE
        .get_or_init(|| parse_rel("pwiz/example_data/small_miape.pwiz.1.1.mzML", false))
}

fn anpc_test_mzml() -> &'static MzML {
    TEST_MZML_CACHE.get_or_init(|| parse_rel("crates/parser/data/mzml/test.mzML", false))
}

#[test]
fn pwiz_serializer_mzml_tiny_10_roundtrip_semantic() {
    let src = tiny_pwiz_10();
    let xml = bin_to_mzml(src).expect("bin_to_mzml should succeed");
    let reparsed = parse_xml(&xml, false);
    assert_mzml_semantic_eq(src, &reparsed);
}

#[test]
fn pwiz_serializer_mzml_tiny_11_roundtrip_semantic() {
    let src = tiny_pwiz_11();
    let xml = bin_to_mzml(src).expect("bin_to_mzml should succeed");
    let reparsed = parse_xml(&xml, false);
    assert_mzml_semantic_eq(src, &reparsed);
}

#[test]
fn pwiz_serializer_mzml_small_10_roundtrip_structural() {
    let src = small_pwiz_10();
    let xml = bin_to_mzml(src).expect("bin_to_mzml should succeed");
    let reparsed = parse_xml(&xml, false);
    assert_mzml_structural_eq(src, &reparsed);
}

#[test]
fn pwiz_serializer_mzml_small_11_roundtrip_structural() {
    let src = small_pwiz_11();
    let xml = bin_to_mzml(src).expect("bin_to_mzml should succeed");
    let reparsed = parse_xml(&xml, false);
    assert_mzml_structural_eq(src, &reparsed);
}

#[test]
fn pwiz_serializer_mzml_additional_real_fixtures_roundtrip_semantic() {
    let fixtures = [
        ("tiny.pwiz.1.1.1", tiny_pwiz_111()),
        ("small_miape.pwiz.1.1", small_miape_pwiz_11()),
    ];

    for (label, src) in fixtures {
        assert_semantic_roundtrip_via_xml(src, label);
        assert_semantic_roundtrip_via_b000(src, 9, label);
    }
}

#[test]
fn pwiz_reader_mzml_tiny2_fixture_exposes_known_unresolved_precursor_ref() {
    let mzml = tiny2_pwiz_10();
    let precursor = precursor_list_of_spectrum(spectrum_by_id(mzml, "S2"))
        .and_then(|list| list.precursors.first())
        .expect("tiny2 fixture should retain its known precursor reference");
    assert_eq!(precursor.spectrum_ref.as_deref(), Some("change_me"));
}

#[test]
fn pwiz_serializer_mzml_small_zlib_roundtrip_semantic() {
    let src = small_zlib_pwiz_11();
    assert_semantic_roundtrip_via_xml(src, "small_zlib.pwiz.1.1");
    assert_semantic_roundtrip_via_b000(src, 9, "small_zlib.pwiz.1.1");
}

#[test]
fn pwiz_parser_mzml_small_zlib_decodes_nonempty_compressed_arrays() {
    let mzml = small_zlib_pwiz_11();
    let first_spectrum = spectra(mzml)
        .first()
        .expect("small_zlib fixture should contain at least one spectrum");
    let arrays = spectrum_arrays(first_spectrum);
    assert!(
        arrays
            .iter()
            .any(|array| cv_has_accession(&array.cv_params, "MS:1000574")),
        "small_zlib fixture should expose zlib compression metadata on at least one binaryDataArray"
    );
    for (index, array) in arrays.iter().enumerate() {
        let binary = array
            .binary
            .as_ref()
            .unwrap_or_else(|| panic!("small_zlib spectrum array {index} missing decoded payload"));
        assert!(
            binary_len(binary) > 0,
            "small_zlib spectrum array {index} should decode to a non-empty payload"
        );
    }
}

#[test]
fn pwiz_reader_indexed_mzml_fixture_indices_match_model() {
    let rel = "crates/parser/data/mzml/tiny.pwiz.mzML0.99.10.mzML";
    let indexed = parse_indexed_rel(rel);
    assert_index_offsets_match_model(&indexed, rel);
}

#[test]
fn pwiz_reader_indexed_mzml_test_fixture_preserves_raw_index_entries() {
    let indexed = parse_indexed_rel("crates/parser/data/mzml/test.mzML");
    assert_eq!(indexed.index_list.spectrum.len(), 2);
    assert_eq!(indexed.index_list.chromatogram.len(), 2);
    assert_eq!(
        indexed.index_list.spectrum[0].id_ref.as_deref(),
        Some("scan=1")
    );
    assert_eq!(
        indexed.index_list.spectrum[1].id_ref.as_deref(),
        Some("scan=2")
    );
    assert_eq!(
        indexed.index_list.chromatogram[0].id_ref.as_deref(),
        Some("TIC")
    );
    assert_eq!(
        indexed.index_list.chromatogram[1].id_ref.as_deref(),
        Some("BPC")
    );
    assert!(indexed
        .index_list
        .spectrum
        .iter()
        .all(|offset| offset.offset > 0));
    assert!(indexed
        .index_list
        .chromatogram
        .iter()
        .all(|offset| offset.offset > 0));
    assert!(indexed.index_list_offset.is_some());
}

#[test]
fn pwiz_serializer_mzml_emits_parseable_index_entries() {
    let fixtures = [
        ("tiny.pwiz.1.1", tiny_pwiz_11()),
        ("small.pwiz.1.1", small_pwiz_11()),
        ("small_zlib.pwiz.1.1", small_zlib_pwiz_11()),
    ];

    for (label, src) in fixtures {
        let xml =
            bin_to_mzml(src).unwrap_or_else(|e| panic!("bin_to_mzml failed for {label}: {e}"));
        let indexed = parse_indexed_mzml(xml.as_bytes())
            .unwrap_or_else(|e| panic!("parse_indexed_mzml failed for generated {label}: {e}"));
        assert_mzml_semantic_eq(src, &indexed.mzml);
        assert_index_offsets_match_model(&indexed, label);
        assert!(
            indexed.index_list_offset.is_some(),
            "generated indexed mzML should include indexListOffset for {label}"
        );
    }
}

#[test]
fn pwiz_numeric_matrix_b000_roundtrip_preserves_rare_numeric_types() {
    let cases = [
        (
            NumericType::Float16,
            BinaryData::F16(vec![0x0000, 0x3c00, 0x4000]),
            BinaryData::F16(vec![0x0000, 0x3555, 0x3c00]),
        ),
        (
            NumericType::Int16,
            BinaryData::I16(vec![-10, 0, 10]),
            BinaryData::I16(vec![-20, 0, 20]),
        ),
        (
            NumericType::Int32,
            BinaryData::I32(vec![-1_000, 0, 1_000]),
            BinaryData::I32(vec![-2_000, 0, 2_000]),
        ),
        (
            NumericType::Int64,
            BinaryData::I64(vec![-1_000_000, 0, 1_000_000]),
            BinaryData::I64(vec![-2_000_000, 0, 2_000_000]),
        ),
    ];

    for (numeric_type, spectrum_binary, chromatogram_binary) in cases {
        let src = synthetic_numeric_matrix_mzml(
            numeric_type,
            spectrum_binary,
            chromatogram_binary,
            Some(3),
        );
        let out = decode(&encode_bytes(&src, 9, false)).expect("decode should succeed");
        assert_mzml_semantic_eq(&src, &out);
    }
}

#[test]
fn pwiz_numeric_matrix_xml_roundtrip_preserves_rare_numeric_types_without_array_length() {
    let cases = [
        (
            NumericType::Float16,
            BinaryData::F16(vec![0x0000, 0x3c00, 0x4000]),
            BinaryData::F16(vec![0x0000, 0x3555, 0x3c00]),
        ),
        (
            NumericType::Int16,
            BinaryData::I16(vec![-10, 0, 10]),
            BinaryData::I16(vec![-20, 0, 20]),
        ),
        (
            NumericType::Int32,
            BinaryData::I32(vec![-1_000, 0, 1_000]),
            BinaryData::I32(vec![-2_000, 0, 2_000]),
        ),
        (
            NumericType::Int64,
            BinaryData::I64(vec![-1_000_000, 0, 1_000_000]),
            BinaryData::I64(vec![-2_000_000, 0, 2_000_000]),
        ),
    ];

    for (numeric_type, spectrum_binary, chromatogram_binary) in cases {
        let src =
            synthetic_numeric_matrix_mzml(numeric_type, spectrum_binary, chromatogram_binary, None);
        let xml = bin_to_mzml(&src).expect("bin_to_mzml should succeed");
        let reparsed = parse_xml(&xml, false);
        assert_mzml_semantic_eq(&src, &reparsed);
    }
}

#[test]
fn pwiz_numeric_matrix_parser_honors_declared_shorter_array_length() {
    let cases = [
        (
            NumericType::Float16,
            BinaryData::F16(vec![0x0000, 0x3c00, 0x4000]),
        ),
        (NumericType::Int16, BinaryData::I16(vec![-10, 0, 10])),
        (NumericType::Int32, BinaryData::I32(vec![-1_000, 0, 1_000])),
        (
            NumericType::Int64,
            BinaryData::I64(vec![-1_000_000, 0, 1_000_000]),
        ),
    ];

    for (numeric_type, binary) in cases {
        let xml = single_array_xml("MS:1000514", numeric_type, &binary, Some(2));
        let mzml = parse_xml(&xml, false);
        let spectrum = spectrum_by_id(&mzml, "scan=1");
        let array = &spectrum_arrays(spectrum)[0];

        assert_eq!(array.numeric_type, Some(numeric_type));
        assert_eq!(array.array_length, Some(2));
        assert_eq!(
            binary_len(array.binary.as_ref().expect("decoded binary present")),
            2,
            "parser should truncate payload to declared arrayLength for {numeric_type:?}"
        );
    }
}

#[test]
fn pwiz_inheritance_default_data_processing_refs_roundtrip_semantic() {
    let encoded = BASE64_STANDARD.encode(binary_to_le_bytes(&BinaryData::F64(vec![1.0, 2.0, 3.0])));
    let xml = format!(
        concat!(
            "<mzML>",
            "{cv_list}",
            "<fileDescription><fileContent/><sourceFileList count=\"0\"/></fileDescription>",
            "<dataProcessingList count=\"2\">",
            "<dataProcessing id=\"dp_default\"><processingMethod order=\"0\"></processingMethod></dataProcessing>",
            "<dataProcessing id=\"dp_override\"><processingMethod order=\"0\"></processingMethod></dataProcessing>",
            "</dataProcessingList>",
            "<run id=\"dp-fallback\">",
            "<spectrumList count=\"1\" defaultDataProcessingRef=\"dp_default\">",
            "<spectrum index=\"0\" id=\"scan=1\" defaultArrayLength=\"3\">",
            "<binaryDataArrayList count=\"2\">",
            "<binaryDataArray arrayLength=\"3\" encodedLength=\"{len}\">",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000514\" name=\"m/z array\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000523\" name=\"64-bit float\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000576\" name=\"no compression\"/>",
            "<binary>{encoded}</binary></binaryDataArray>",
            "<binaryDataArray arrayLength=\"3\" encodedLength=\"{len}\" dataProcessingRef=\"dp_override\">",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000515\" name=\"intensity array\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000523\" name=\"64-bit float\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000576\" name=\"no compression\"/>",
            "<binary>{encoded}</binary></binaryDataArray>",
            "</binaryDataArrayList></spectrum></spectrumList>",
            "<chromatogramList count=\"1\" defaultDataProcessingRef=\"dp_default\">",
            "<chromatogram index=\"0\" id=\"tic\" defaultArrayLength=\"3\">",
            "<binaryDataArrayList count=\"1\">",
            "<binaryDataArray arrayLength=\"3\" encodedLength=\"{len}\">",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000595\" name=\"time array\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000523\" name=\"64-bit float\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000576\" name=\"no compression\"/>",
            "<binary>{encoded}</binary></binaryDataArray>",
            "</binaryDataArrayList></chromatogram></chromatogramList>",
            "</run></mzML>"
        ),
        cv_list = DEFAULT_CV_LIST_XML,
        len = encoded.len(),
        encoded = encoded,
    );

    let mzml = parse_xml(&xml, false);
    let spectrum_list = mzml
        .run
        .spectrum_list
        .as_ref()
        .expect("spectrumList parsed");
    let spectrum = &spectrum_list.spectra[0];
    let arrays = spectrum_arrays(spectrum);
    assert_eq!(
        spectrum_list.default_data_processing_ref.as_deref(),
        Some("dp_default")
    );
    assert_eq!(arrays[0].data_processing_ref, None);
    assert_eq!(
        arrays[1].data_processing_ref.as_deref(),
        Some("dp_override")
    );

    assert_semantic_roundtrip_via_xml(&mzml, "dp-fallback-xml");
    assert_semantic_roundtrip_via_b000(&mzml, 9, "dp-fallback-b000");
}

#[test]
fn pwiz_inheritance_scan_settings_and_source_file_refs_roundtrip_semantic() {
    let xml = format!(
        r#"
<mzML>
  {cv_list}
  <fileDescription>
    <fileContent/>
    <sourceFileList count="1">
      <sourceFile id="SF1" name="input.raw" location="file:///tmp/input.raw"/>
    </sourceFileList>
  </fileDescription>
  <scanSettingsList count="1">
    <scanSettings id="SS1" instrumentConfigurationRef="IC1">
      <sourceFileRefList count="1">
        <sourceFileRef ref="SF1"/>
      </sourceFileRefList>
      <targetList count="1">
        <target>
          <userParam name="active time" type="seconds" value="0.5"/>
        </target>
      </targetList>
    </scanSettings>
  </scanSettingsList>
  <instrumentConfigurationList count="1">
    <instrumentConfiguration id="IC1" scanSettingsRef="SS1"/>
  </instrumentConfigurationList>
  <run id="scan-settings" defaultInstrumentConfigurationRef="IC1" defaultSourceFileRef="SF1">
    <sourceFileRefList count="1">
      <sourceFileRef ref="SF1"/>
    </sourceFileRefList>
    <spectrumList count="1">
      <spectrum index="0" id="scan=1" defaultArrayLength="0">
        <scanList count="1">
          <scan instrumentConfigurationRef="IC1"/>
        </scanList>
      </spectrum>
    </spectrumList>
    <chromatogramList count="0"/>
  </run>
</mzML>
"#,
        cv_list = DEFAULT_CV_LIST_XML
    );

    let mzml = parse_xml(&xml, false);
    assert_eq!(mzml.run.default_source_file_ref.as_deref(), Some("SF1"));
    assert_eq!(
        mzml.instrument_list
            .as_ref()
            .expect("instrument list parsed")
            .instrument[0]
            .scan_settings_ref
            .as_ref()
            .map(|value| value.r#ref.as_str()),
        Some("SS1")
    );

    assert_semantic_roundtrip_via_xml(&mzml, "scan-settings-source-files-xml");
    assert_semantic_roundtrip_via_b000(&mzml, 9, "scan-settings-source-files-b000");
}

#[test]
fn pwiz_inheritance_instrument_software_ref_roundtrip_semantic() {
    let xml = format!(
        r#"
<mzML>
  {cv_list}
  <fileDescription>
    <fileContent/>
    <sourceFileList count="0"/>
  </fileDescription>
  <softwareList count="2">
    <software id="legacy-sw" version="0.1">
      <cvParam cvRef="MS" accession="MS:1000531" name="software" value=""/>
    </software>
    <software id="acq-sw" version="1.0">
      <cvParam cvRef="MS" accession="MS:1000531" name="software" value=""/>
    </software>
  </softwareList>
  <instrumentConfigurationList count="1">
    <instrumentConfiguration id="IC1" softwareRef="legacy-sw">
      <softwareRef ref="acq-sw"/>
    </instrumentConfiguration>
  </instrumentConfigurationList>
  <run id="instrument-software-ref" defaultInstrumentConfigurationRef="IC1">
    <spectrumList count="1">
      <spectrum index="0" id="scan=1" defaultArrayLength="0">
        <scanList count="1">
          <scan/>
        </scanList>
      </spectrum>
    </spectrumList>
    <chromatogramList count="0"/>
  </run>
</mzML>
"#,
        cv_list = DEFAULT_CV_LIST_XML
    );

    let mzml = parse_xml(&xml, false);
    assert_eq!(
        mzml.instrument_list
            .as_ref()
            .expect("instrument list parsed")
            .instrument[0]
            .software_ref
            .as_ref()
            .map(|value| value.r#ref.as_str()),
        Some("acq-sw")
    );

    assert_semantic_roundtrip_via_xml(&mzml, "instrument-software-ref-xml");
    assert_semantic_roundtrip_via_b000(&mzml, 9, "instrument-software-ref-b000");
    assert_semantic_roundtrip_full_pipeline(&mzml, 9, "instrument-software-ref-full");
}

#[test]
fn pwiz_inheritance_legacy_spectrum_description_roundtrip_semantic() {
    let xml = format!(
        r#"
<mzML>
  {cv_list}
  <fileDescription>
    <fileContent/>
    <sourceFileList count="0"/>
  </fileDescription>
  <run id="legacy-spectrum-description">
    <spectrumList count="2">
      <spectrum index="0" id="S0" defaultArrayLength="0">
        <spectrumDescription>
          <scanList count="1"><scan/></scanList>
        </spectrumDescription>
      </spectrum>
      <spectrum index="1" id="S1" defaultArrayLength="0">
        <spectrumDescription>
          <scanList count="1"><scan/></scanList>
          <precursorList count="1">
            <precursor spectrumRef="S0">
              <selectedIonList count="1">
                <selectedIon>
                  <cvParam cvRef="MS" accession="MS:1000744" name="selected ion m/z" value="445.34"/>
                </selectedIon>
              </selectedIonList>
            </precursor>
          </precursorList>
          <productList count="1">
            <product>
              <isolationWindow>
                <cvParam cvRef="MS" accession="MS:1000827" name="isolation window target m/z" value="100.0"/>
              </isolationWindow>
            </product>
          </productList>
        </spectrumDescription>
      </spectrum>
    </spectrumList>
    <chromatogramList count="0"/>
  </run>
</mzML>
"#,
        cv_list = DEFAULT_CV_LIST_XML
    );

    let mzml = parse_xml(&xml, false);
    assert_eq!(spectra(&mzml).len(), 2);
    assert!(spectrum_by_id(&mzml, "S1").spectrum_description.is_some());

    assert_semantic_roundtrip_via_xml(&mzml, "legacy-spectrum-description-xml");
    assert_semantic_roundtrip_via_b000(&mzml, 9, "legacy-spectrum-description-b000");
}

#[test]
fn pwiz_semantic_audit_ref_groups_and_list_metadata_survive_full_pipeline() {
    let xml = format!(
        r#"
<mzML>
  {cv_list}
  <referenceableParamGroupList count="8">
    <referenceableParamGroup id="fc-group"><userParam name="fc-note" value="file-content" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="sf-group"><userParam name="sf-note" value="source-file" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="contact-group"><userParam name="contact-note" value="contact" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="chrom-group"><userParam name="chrom-note" value="chrom" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="iw-group"><userParam name="iw-note" value="isolation-window" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="act-group"><userParam name="act-note" value="activation" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="si-group"><userParam name="si-note" value="selected-ion" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="target-group"><userParam name="target-note" value="target" type="xsd:string"/></referenceableParamGroup>
  </referenceableParamGroupList>
  <fileDescription>
    <fileContent>
      <referenceableParamGroupRef ref="fc-group"/>
    </fileContent>
    <sourceFileList count="1">
      <sourceFile id="SF1" name="input.raw" location="file:///tmp/input.raw">
        <referenceableParamGroupRef ref="sf-group"/>
      </sourceFile>
    </sourceFileList>
    <contact>
      <referenceableParamGroupRef ref="contact-group"/>
    </contact>
  </fileDescription>
  <scanSettingsList count="1">
    <scanSettings id="SS1">
      <targetList count="1">
        <target>
          <referenceableParamGroupRef ref="target-group"/>
        </target>
      </targetList>
    </scanSettings>
  </scanSettingsList>
  <run id="semantic-audit" defaultSourceFileRef="SF1">
    <sourceFileRefList count="1">
      <sourceFileRef ref="SF1"/>
    </sourceFileRefList>
    <spectrumList count="1">
      <spectrum index="0" id="scan=1" defaultArrayLength="0">
        <precursorList count="1">
          <userParam name="precursor-list-note" value="keep-me" type="xsd:string"/>
          <precursor spectrumRef="scan=1" sourceFileRef="SF1">
            <isolationWindow>
              <referenceableParamGroupRef ref="iw-group"/>
            </isolationWindow>
            <selectedIonList count="1">
              <selectedIon>
                <referenceableParamGroupRef ref="si-group"/>
              </selectedIon>
            </selectedIonList>
            <activation>
              <referenceableParamGroupRef ref="act-group"/>
            </activation>
          </precursor>
        </precursorList>
        <productList count="1">
          <userParam name="product-list-note" value="keep-me-too" type="xsd:string"/>
          <product sourceFileRef="SF1">
            <isolationWindow>
              <referenceableParamGroupRef ref="iw-group"/>
            </isolationWindow>
          </product>
        </productList>
      </spectrum>
    </spectrumList>
    <chromatogramList count="1">
      <chromatogram index="0" id="tic" defaultArrayLength="0">
        <referenceableParamGroupRef ref="chrom-group"/>
        <precursor spectrumRef="scan=1" sourceFileRef="SF1">
          <isolationWindow>
            <referenceableParamGroupRef ref="iw-group"/>
          </isolationWindow>
          <selectedIonList count="1">
            <selectedIon>
              <referenceableParamGroupRef ref="si-group"/>
            </selectedIon>
          </selectedIonList>
          <activation>
            <referenceableParamGroupRef ref="act-group"/>
          </activation>
        </precursor>
        <product sourceFileRef="SF1">
          <isolationWindow>
            <referenceableParamGroupRef ref="iw-group"/>
          </isolationWindow>
        </product>
      </chromatogram>
    </chromatogramList>
  </run>
</mzML>
"#,
        cv_list = DEFAULT_CV_LIST_XML
    );

    let mzml = parse_xml(&xml, false);
    assert_semantic_roundtrip_full_pipeline(&mzml, 9, "semantic-audit-full-pipeline");
}

#[test]
#[ignore = "manual semantic audit on medium fixture; parser still loads whole file into RAM"]
fn pwiz_medium_fixture_manual_semantic_audit_full_pipeline() {
    let mzml = parse_rel(
        "inputs/covid19_biogune_MS_AA_PAI04_COVp20_220121_COV02001_19S20575_21.mzML",
        false,
    );

    assert_semantic_roundtrip_full_pipeline(&mzml, 12, "medium-covid19-full-pipeline");
}

#[test]
#[ignore = "manual memory-heavy smoke; parser currently loads the full file into RAM"]
fn pwiz_medium_fixture_manual_parse_smoke() {
    let mzml = parse_rel(
        "inputs/covid19_biogune_MS_AA_PAI04_COVp20_220121_COV02001_19S20575_21.mzML",
        false,
    );

    assert_declared_counts_consistent(&mzml);
    assert!(
        !spectra(&mzml).is_empty(),
        "medium fixture should contain spectra"
    );
    assert_all_refs_resolved(&mzml);
}

#[test]
fn pwiz_serializer_mzml_b000_roundtrip_tiny_11_level12() {
    let src = tiny_pwiz_11();
    let bytes = encode_bytes(src, 12, false);
    assert_eq!(
        &bytes[..4],
        b"B000",
        "encoded header signature must be B000"
    );

    let decoded = decode(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(src, &decoded);
}

#[test]
fn pwiz_serializer_mzml_b000_roundtrip_tiny_10_level0() {
    let src = tiny_pwiz_10();
    let bytes = encode_bytes(src, 0, false);
    assert_eq!(
        &bytes[..4],
        b"B000",
        "encoded header signature must be B000"
    );

    let decoded = decode(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(src, &decoded);
}

#[test]
fn pwiz_serializer_mzml_b000_f32_mode_keeps_structure() {
    let src = tiny_pwiz_11();
    let bytes = encode_bytes(src, 9, true);
    let decoded = decode(&bytes).expect("decode should succeed");

    assert_mzml_structural_eq(src, &decoded);
}

#[test]
fn pwiz_serializer_mzml_b000_config_matrix_identity_smoke() {
    let fixtures = [tiny_pwiz_10(), tiny_pwiz_11(), anpc_test_mzml()];
    let levels = [0_u8, 3_u8, 6_u8, 9_u8, 12_u8];
    let modes = [false, true];

    for (fi, src) in fixtures.into_iter().enumerate() {
        for level in levels {
            for to_f32 in modes {
                let bytes = encode_bytes(src, level, to_f32);
                let out = decode(&bytes).expect("decode should succeed");

                let src_spec_ids: Vec<_> = spectra(src).iter().map(|s| s.id.as_str()).collect();
                let out_spec_ids: Vec<_> = spectra(&out).iter().map(|s| s.id.as_str()).collect();
                assert_eq!(
                    src_spec_ids, out_spec_ids,
                    "spectrum ids changed in matrix case fixture#{fi} level={level} f32={to_f32}"
                );

                let src_chrom_ids: Vec<_> =
                    chromatograms(src).iter().map(|c| c.id.as_str()).collect();
                let out_chrom_ids: Vec<_> =
                    chromatograms(&out).iter().map(|c| c.id.as_str()).collect();
                assert_eq!(
                    src_chrom_ids, out_chrom_ids,
                    "chrom ids changed in matrix case fixture#{fi} level={level} f32={to_f32}"
                );
            }
        }
    }
}

#[test]
fn pwiz_spectrum_list_mzml_tiny_11_identity_and_counts() {
    let mzml = tiny_pwiz_11();
    let sl = mzml
        .run
        .spectrum_list
        .as_ref()
        .expect("spectrumList parsed");
    assert_eq!(sl.spectra.len(), 4);

    let ids: Vec<_> = sl.spectra.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids[0], "scan=19");
    assert_eq!(ids[1], "scan=20");
    assert_eq!(ids[2], "scan=21");
    assert_eq!(ids[3], "sample=1 period=1 cycle=22 experiment=1");

    assert_eq!(sl.spectra[3].spot_id.as_deref(), Some("A1,42x42,4242x4242"));
}

#[test]
fn pwiz_spectrum_list_mzml_tiny_10_identity_and_counts() {
    let mzml = tiny_pwiz_10();
    let sl = mzml
        .run
        .spectrum_list
        .as_ref()
        .expect("spectrumList parsed");
    assert_eq!(sl.spectra.len(), 4);

    let ids: Vec<_> = sl.spectra.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["S19", "S20", "S21", "S22"]);
    assert_eq!(sl.spectra[3].spot_id.as_deref(), Some("A1,42x42,4242x4242"));
}

#[test]
fn pwiz_spectrum_list_mzml_small_11_first_last_identity() {
    let mzml = small_pwiz_11();
    let sl = mzml
        .run
        .spectrum_list
        .as_ref()
        .expect("spectrumList parsed");
    assert_eq!(sl.spectra.len(), 48);

    assert_eq!(sl.spectra.first().map(|s| s.index), Some(Some(0)));
    assert_eq!(
        sl.spectra.first().map(|s| s.id.as_str()),
        Some("controllerType=0 controllerNumber=1 scan=1")
    );

    assert_eq!(sl.spectra.last().map(|s| s.index), Some(Some(47)));
    assert_eq!(
        sl.spectra.last().map(|s| s.id.as_str()),
        Some("controllerType=0 controllerNumber=1 scan=48")
    );
}

#[test]
fn pwiz_spectrum_list_mzml_tiny_11_binary_payload_shapes() {
    let mzml = tiny_pwiz_11();

    let s19 = spectrum_by_id(mzml, "scan=19");
    assert_eq!(s19.default_array_length, Some(15));
    let a19 = spectrum_arrays(s19);
    assert_eq!(a19.len(), 2);
    let mz19 = find_array_by_accession(a19, "MS:1000514");
    let int19 = find_array_by_accession(a19, "MS:1000515");
    assert_eq!(
        first_f64_from_binary(mz19.binary.as_ref().expect("mz binary")),
        Some(0.0)
    );
    assert_eq!(
        first_f64_from_binary(int19.binary.as_ref().expect("intensity binary")),
        Some(15.0)
    );

    let s20 = spectrum_by_id(mzml, "scan=20");
    assert_eq!(s20.default_array_length, Some(10));
    let a20 = spectrum_arrays(s20);
    assert_eq!(a20.len(), 2);
    let mz20 = find_array_by_accession(a20, "MS:1000514");
    let int20 = find_array_by_accession(a20, "MS:1000515");
    assert_eq!(
        first_f64_from_binary(mz20.binary.as_ref().expect("mz binary")),
        Some(0.0)
    );
    assert_eq!(
        first_f64_from_binary(int20.binary.as_ref().expect("intensity binary")),
        Some(20.0)
    );
}

#[test]
fn pwiz_spectrum_list_mzml_tiny_11_precursor_ref_integrity() {
    let mzml = tiny_pwiz_11();
    let s20 = spectrum_by_id(mzml, "scan=20");

    let precursor_list = s20
        .precursor_list
        .as_ref()
        .expect("scan=20 precursorList parsed");
    assert_eq!(precursor_list.precursors.len(), 1);

    let p0 = &precursor_list.precursors[0];
    assert_eq!(p0.spectrum_ref.as_deref(), Some("scan=19"));
}

#[test]
fn pwiz_spectrum_list_mzml_tiny_11_find_name_value_equivalence() {
    let mzml = tiny_pwiz_11();
    let sl = mzml
        .run
        .spectrum_list
        .as_ref()
        .expect("spectrumList parsed");

    let by_id = |id: &str| sl.spectra.iter().position(|s| s.id == id);

    assert_eq!(by_id("scan=19"), Some(0));
    assert_eq!(by_id("scan=20"), Some(1));
    assert_eq!(by_id("scan=21"), Some(2));
    assert_eq!(by_id("sample=1 period=1 cycle=22 experiment=1"), Some(3));

    assert_eq!(find_name_value_indices(mzml, "scan", "19"), vec![0]);
    assert_eq!(find_name_value_indices(mzml, "scan", "20"), vec![1]);
    assert_eq!(find_name_value_indices(mzml, "scan", "21"), vec![2]);
    assert_eq!(find_name_value_indices(mzml, "sample", "1"), vec![3]);
    assert_eq!(find_name_value_indices(mzml, "period", "1"), vec![3]);
    assert_eq!(find_name_value_indices(mzml, "cycle", "22"), vec![3]);
    assert_eq!(find_name_value_indices(mzml, "experiment", "1"), vec![3]);
}

#[test]
fn pwiz_spectrum_list_mzml_tiny_11_spot_id_lookup_equivalence() {
    let mzml = tiny_pwiz_11();
    assert!(
        find_spot_id_indices(mzml, "A1").is_empty(),
        "partial spot id should not match"
    );
    assert_eq!(find_spot_id_indices(mzml, "A1,42x42,4242x4242"), vec![3]);
}

#[test]
fn pwiz_spectrum_list_mzml_tiny_11_scan_param_group_ref_equivalence() {
    let mzml = tiny_pwiz_11();
    let s19 = spectrum_by_id(mzml, "scan=19");
    let s20 = spectrum_by_id(mzml, "scan=20");

    assert_eq!(s19.referenceable_param_group_refs.len(), 1);
    assert_eq!(
        s19.referenceable_param_group_refs[0].r#ref,
        "CommonMS1SpectrumParams"
    );

    assert_eq!(s20.referenceable_param_group_refs.len(), 1);
    assert_eq!(
        s20.referenceable_param_group_refs[0].r#ref,
        "CommonMS2SpectrumParams"
    );
}

#[test]
fn pwiz_spectrum_list_mzml_tiny_11_s19_pairwise_binary_values() {
    let mzml = tiny_pwiz_11();
    let s19 = spectrum_by_id(mzml, "scan=19");

    let mz = first_array_values_by_accession(s19, "MS:1000514");
    let intensity = first_array_values_by_accession(s19, "MS:1000515");
    assert_eq!(mz.len(), 15);
    assert_eq!(intensity.len(), 15);

    for i in 0..15 {
        rel_close_f64(mz[i], i as f64, EPS_REL_F64, &format!("scan=19 mz[{i}]"));
        rel_close_f64(
            intensity[i],
            (15 - i) as f64,
            EPS_REL_F64,
            &format!("scan=19 intensity[{i}]"),
        );
    }
}

#[test]
fn pwiz_spectrum_list_mzml_tiny_11_s20_pairwise_binary_values() {
    let mzml = tiny_pwiz_11();
    let s20 = spectrum_by_id(mzml, "scan=20");

    let mz = first_array_values_by_accession(s20, "MS:1000514");
    let intensity = first_array_values_by_accession(s20, "MS:1000515");
    assert_eq!(mz.len(), 10);
    assert_eq!(intensity.len(), 10);

    for i in 0..10 {
        rel_close_f64(
            mz[i],
            (2 * i) as f64,
            EPS_REL_F64,
            &format!("scan=20 mz[{i}]"),
        );
        rel_close_f64(
            intensity[i],
            (2 * (10 - i)) as f64,
            EPS_REL_F64,
            &format!("scan=20 intensity[{i}]"),
        );
    }
}

#[test]
fn pwiz_chromatogram_list_mzml_tiny_11_identity_and_shapes() {
    let mzml = tiny_pwiz_11();
    let cl = mzml
        .run
        .chromatogram_list
        .as_ref()
        .expect("chromatogramList parsed");

    assert_eq!(cl.chromatograms.len(), 2);

    let tic = chromatogram_by_id(mzml, "tic");
    assert_eq!(tic.default_array_length, Some(15));
    assert_eq!(chromatogram_arrays(tic).len(), 2);

    let sic = chromatogram_by_id(mzml, "sic");
    assert_eq!(sic.default_array_length, Some(10));
    assert_eq!(chromatogram_arrays(sic).len(), 2);
}

#[test]
fn pwiz_chromatogram_list_mzml_tiny_10_identity_and_native_ids() {
    let mzml = tiny_pwiz_10();

    let tic = chromatogram_by_id(mzml, "tic");
    assert_eq!(tic.native_id.as_deref(), Some("tic native"));
    assert_eq!(tic.default_array_length, Some(15));

    let sic = chromatogram_by_id(mzml, "sic");
    assert_eq!(sic.native_id.as_deref(), Some("sic native"));
    assert_eq!(sic.default_array_length, Some(10));
}

#[test]
fn pwiz_chromatogram_list_mzml_small_11_identity() {
    let mzml = small_pwiz_11();
    let cl = mzml
        .run
        .chromatogram_list
        .as_ref()
        .expect("chromatogramList parsed");

    assert_eq!(cl.chromatograms.len(), 1);
    assert_eq!(cl.chromatograms[0].id, "TIC");
    assert_eq!(cl.chromatograms[0].default_array_length, Some(48));
}

#[test]
fn pwiz_chromatogram_list_mzml_tiny_11_tic_pairwise_values() {
    let mzml = tiny_pwiz_11();
    let tic = chromatogram_by_id(mzml, "tic");

    let t = first_chrom_array_values_by_accession(tic, "MS:1000595");
    let i = first_chrom_array_values_by_accession(tic, "MS:1000515");
    assert_eq!(t.len(), 15);
    assert_eq!(i.len(), 15);

    for idx in 0..15 {
        rel_close_f64(t[idx], idx as f64, EPS_REL_F64, &format!("tic time[{idx}]"));
        rel_close_f64(
            i[idx],
            (15 - idx) as f64,
            EPS_REL_F64,
            &format!("tic intensity[{idx}]"),
        );
    }
}

#[test]
fn pwiz_chromatogram_list_mzml_tiny_11_sic_pairwise_values() {
    let mzml = tiny_pwiz_11();
    let sic = chromatogram_by_id(mzml, "sic");

    let t = first_chrom_array_values_by_accession(sic, "MS:1000595");
    let i = first_chrom_array_values_by_accession(sic, "MS:1000515");
    assert_eq!(t.len(), 10);
    assert_eq!(i.len(), 10);

    for idx in 0..10 {
        rel_close_f64(t[idx], idx as f64, EPS_REL_F64, &format!("sic time[{idx}]"));
        rel_close_f64(
            i[idx],
            (10 - idx) as f64,
            EPS_REL_F64,
            &format!("sic intensity[{idx}]"),
        );
    }
}

#[test]
fn pwiz_msdatafile_mzml_subset_parse_encode_decode_tiny11() {
    let src = tiny_pwiz_11();
    let bytes = encode_bytes(src, 12, false);
    let out = decode(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(src, &out);
}

#[test]
fn pwiz_msdatafile_mzml_subset_parse_encode_decode_tiny10() {
    let src = tiny_pwiz_10();
    let bytes = encode_bytes(src, 7, false);
    let out = decode(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(src, &out);
}

#[test]
fn pwiz_msdatafile_mzml_subset_parse_bin_to_mzml_parse_then_b000_roundtrip() {
    let src = tiny_pwiz_11();
    let xml = bin_to_mzml(src).expect("bin_to_mzml should succeed");
    let reparsed = parse_xml(&xml, false);

    let bytes = encode_bytes(&reparsed, 6, false);
    let decoded = decode(&bytes).expect("decode should succeed");

    assert_mzml_semantic_eq(&reparsed, &decoded);
}

#[test]
fn pwiz_msdatafile_mzml_subset_repeated_roundtrip_stability() {
    let src = tiny_pwiz_11();
    let mut last_fp = semantic_fingerprint(src);

    for iter in 0..25 {
        let bytes = encode_bytes(src, 9, false);
        let out = decode(&bytes).expect("decode should succeed");
        let fp = semantic_fingerprint(&out);
        assert_eq!(fp, last_fp, "semantic fingerprint changed at iter={iter}");
        last_fp = fp;
    }
}

#[test]
fn pwiz_reader_mzml_route_can_parse_all_mzml_fixtures() {
    let fixtures = [
        "pwiz/example_data/tiny.pwiz.1.0.mzML",
        "pwiz/example_data/tiny.pwiz.1.1.mzML",
        "pwiz/example_data/small.pwiz.1.0.mzML",
        "pwiz/example_data/small.pwiz.1.1.mzML",
        "crates/parser/data/mzml/test.mzML",
        "crates/parser/data/mzml/tiny.msdata.mzML0.99.10.mzML",
        "crates/parser/data/mzml/tiny.pwiz.mzML0.99.10.mzML",
    ];

    for rel in fixtures {
        let mzml = parse_rel(rel, false);
        assert!(
            mzml.run
                .spectrum_list
                .as_ref()
                .map(|sl| !sl.spectra.is_empty())
                .unwrap_or(false),
            "fixture {rel} should have non-empty spectrumList"
        );
    }
}

#[test]
fn pwiz_reader_mzml_route_accepts_minimal_mzml_document() {
    let xml = r#"
<mzML>
  <fileDescription>
    <fileContent/>
    <sourceFileList count="0"/>
  </fileDescription>
  <run id="minimal"/>
</mzML>
"#;
    let m = parse_xml(xml, false);
    assert!(m.run.spectrum_list.is_none());
    assert!(m.run.chromatogram_list.is_none());
}

#[test]
fn pwiz_reader_mzml_route_non_mzml_root_is_graceful() {
    let mzxml_like = r#"<mzXML><msRun/></mzXML>"#;
    let parsed = parse_mzml(mzxml_like.as_bytes()).expect("parser should be graceful");
    assert_eq!(parsed.run.id, "");
    assert!(parsed.run.spectrum_list.is_none());
    assert!(parsed.run.chromatogram_list.is_none());
}

#[test]
fn pwiz_reader_mzml_route_slim_flag_preserves_identity_until_specialized_mode_returns() {
    let fixtures = [
        "pwiz/example_data/tiny.pwiz.1.0.mzML",
        "pwiz/example_data/tiny.pwiz.1.1.mzML",
        "pwiz/example_data/small.pwiz.1.1.mzML",
    ];

    for rel in fixtures {
        let full = parse_rel(rel, false);
        let slim = parse_rel(rel, true);

        assert_eq!(full.run.id, slim.run.id, "slim changed run id for {rel}");
        assert_eq!(
            top_level_source_file_ids(&full),
            top_level_source_file_ids(&slim),
            "slim changed sourceFile ids for {rel}"
        );
        assert_eq!(
            top_level_software_ids(&full),
            top_level_software_ids(&slim),
            "slim changed software ids for {rel}"
        );

        assert_mzml_structural_eq(&full, &slim);
    }
}

#[test]
fn pwiz_spectrum_list_cache_mzml_subset_repeated_spectrum_access_is_stable() {
    let mzml = tiny_pwiz_11();
    let ctx = SemanticCtx::new(mzml);
    let sl = mzml
        .run
        .spectrum_list
        .as_ref()
        .expect("spectrumList parsed");

    let baseline = sl.spectra[0].clone();
    for i in 0..200 {
        let current = &sl.spectra[0];
        assert_spectrum_semantic_eq(
            &ctx,
            &ctx,
            &baseline,
            current,
            sl.default_data_processing_ref.as_deref(),
            sl.default_data_processing_ref.as_deref(),
            &format!("repeated spectrum read iter {i}"),
        );
    }
}

#[test]
fn pwiz_spectrum_list_cache_mzml_subset_repeated_chrom_access_is_stable() {
    let mzml = tiny_pwiz_11();
    let ctx = SemanticCtx::new(mzml);
    let cl = mzml
        .run
        .chromatogram_list
        .as_ref()
        .expect("chromatogramList parsed");

    let baseline = cl.chromatograms[0].clone();
    for i in 0..200 {
        let current = &cl.chromatograms[0];
        assert_chromatogram_semantic_eq(
            &ctx,
            &ctx,
            &baseline,
            current,
            cl.default_data_processing_ref.as_deref(),
            cl.default_data_processing_ref.as_deref(),
            &format!("repeated chromatogram read iter {i}"),
        );
    }
}

#[test]
fn pwiz_binary_data_encoder_b000_header_signature_is_correct() {
    let bytes = encode_bytes(tiny_pwiz_11(), 12, false);
    assert!(
        bytes.len() > 512,
        "encoded bytes should include header and payload"
    );
    assert_eq!(&bytes[..4], b"B000");
}

#[test]
fn pwiz_binary_data_encoder_deterministic_for_same_input_and_config() {
    let src = tiny_pwiz_11();
    let a = encode_bytes(src, 9, false);
    let b = encode_bytes(src, 9, false);
    assert_eq!(a, b, "encode must be deterministic for same input/config");
}

#[test]
fn pwiz_binary_data_encoder_roundtrip_across_levels() {
    let src = tiny_pwiz_11();
    for level in [0_u8, 3_u8, 12_u8] {
        let bytes = encode_bytes(src, level, false);
        let out = decode(&bytes).expect("decode should succeed");
        assert_mzml_semantic_eq(src, &out);
    }
}

#[test]
fn pwiz_binary_data_encoder_preserves_integer_ms_level_arrays() {
    let src = anpc_test_mzml();
    let bytes = encode_bytes(src, 10, false);
    let out = decode(&bytes).expect("decode should succeed");

    let tic = chromatogram_by_id(&out, "TIC");
    let arrays = chromatogram_arrays(tic);
    let ms_level = find_array_by_accession(arrays, "MS:1000786");
    assert_eq!(ms_level.numeric_type, Some(NumericType::Int64));

    match ms_level.binary.as_ref().expect("ms level binary present") {
        BinaryData::I64(v) => {
            assert!(!v.is_empty(), "ms level array must be non-empty");
        }
        other => panic!("ms level array must be I64, got {other:?}"),
    }
}

#[test]
fn pwiz_io_mzml_subset_namespaces_parse_correctly() {
    let m10 = tiny_pwiz_10();
    let m11 = tiny_pwiz_11();

    assert_eq!(spectra(m10).len(), 4);
    assert_eq!(spectra(m11).len(), 4);

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
fn pwiz_io_mzml_subset_roundtrip_xml_parse_stability() {
    let src = tiny_pwiz_11();
    let xml = bin_to_mzml(src).expect("bin_to_mzml should succeed");

    let reparsed_once = parse_xml(&xml, false);
    let xml2 = bin_to_mzml(&reparsed_once).expect("second bin_to_mzml should succeed");
    let reparsed_twice = parse_xml(&xml2, false);

    assert_mzml_semantic_eq(&reparsed_once, &reparsed_twice);
}

#[test]
fn pwiz_io_mzml_slim_flag_preserves_identity_until_specialized_mode_returns() {
    let full = tiny_pwiz_11();
    let slim = parse_rel("pwiz/example_data/tiny.pwiz.1.1.mzML", true);

    assert_eq!(full.run.id, slim.run.id);
    assert_eq!(
        top_level_source_file_ids(full),
        top_level_source_file_ids(&slim)
    );
    assert_eq!(top_level_software_ids(full), top_level_software_ids(&slim));

    assert_mzml_structural_eq(full, &slim);
}

#[test]
fn pwiz_io_mzml_binary_data_array_external_metadata_referenceable_param_group() {
    // Adapted from PWiz IOTest::testBinaryDataArrayExternalMetadata.
    let xml = r#"
<mzML>
  <fileDescription>
    <fileContent/>
    <sourceFileList count="0"/>
  </fileDescription>
  <referenceableParamGroupList count="1">
    <referenceableParamGroup id="mz_params">
      <cvParam cvRef="MS" accession="MS:1000514" name="m/z array"/>
      <cvParam cvRef="MS" accession="MS:1000523" name="64-bit float"/>
      <cvParam cvRef="MS" accession="MS:1000576" name="no compression"/>
    </referenceableParamGroup>
  </referenceableParamGroupList>
  <run id="external-metadata-test">
    <spectrumList count="1">
      <spectrum index="0" id="scan=1" defaultArrayLength="15">
        <binaryDataArrayList count="1">
          <binaryDataArray encodedLength="160" arrayLength="15">
            <referenceableParamGroupRef ref="mz_params"/>
            <binary>AAAAAAAAAAAAAAAAAADwPwAAAAAAAABAAAAAAAAACEAAAAAAAAAQQAAAAAAAABRAAAAAAAAAGEAAAAAAAAAcQAAAAAAAACBAAAAAAAAAIkAAAAAAAAAkQAAAAAAAACZAAAAAAAAAKEAAAAAAAAAqQAAAAAAAACxA</binary>
          </binaryDataArray>
        </binaryDataArrayList>
      </spectrum>
    </spectrumList>
  </run>
</mzML>
"#;

    let mzml = parse_xml(xml, false);
    let s = spectrum_by_id(&mzml, "scan=1");
    let bdal = s
        .binary_data_array_list
        .as_ref()
        .expect("binaryDataArrayList parsed");
    assert_eq!(bdal.binary_data_arrays.len(), 1);

    let bda = &bdal.binary_data_arrays[0];
    assert_eq!(bda.referenceable_param_group_refs.len(), 1);
    assert_eq!(bda.referenceable_param_group_refs[0].r#ref, "mz_params");
    assert_eq!(bda.numeric_type, Some(NumericType::Float64));

    let values = binary_to_f64_vec(bda.binary.as_ref().expect("decoded binary payload present"));
    assert_eq!(values.len(), 15);
    for (i, v) in values.iter().enumerate() {
        rel_close_f64(
            *v,
            i as f64,
            EPS_REL_F64,
            &format!("external metadata bda[{i}]"),
        );
    }
}

#[test]
fn pwiz_diff_mzml_semantic_fingerprint_is_stable_for_identical_input() {
    let a = tiny_pwiz_11().clone();
    let b = tiny_pwiz_11().clone();

    assert_eq!(semantic_fingerprint(&a), semantic_fingerprint(&b));
}

#[test]
fn pwiz_diff_mzml_semantic_fingerprint_changes_on_critical_mutation() {
    let a = tiny_pwiz_11().clone();
    let mut b = tiny_pwiz_11().clone();

    b.run.id.push_str("_mut");
    assert_ne!(semantic_fingerprint(&a), semantic_fingerprint(&b));
    let diffs = canonical_diff_paths(&a, &b);
    assert!(
        diffs.iter().any(|d| d.starts_with("run/id:")),
        "expected run/id in canonical diff, got: {diffs:#?}"
    );
}

#[test]
fn pwiz_diff_mzml_binary_only_equivalence_when_ignoring_identity() {
    let src = tiny_pwiz_11();
    let mut modified = src.clone();

    modified.run.id = "modified-run".to_string();
    if let Some(sl) = modified.run.spectrum_list.as_mut() {
        for (i, s) in sl.spectra.iter_mut().enumerate() {
            s.id = format!("mut-spectrum-{i}");
            s.scan_number = None;
            s.ms_level = None;
        }
    }
    if let Some(cl) = modified.run.chromatogram_list.as_mut() {
        for (i, c) in cl.chromatograms.iter_mut().enumerate() {
            c.id = format!("mut-chromatogram-{i}");
        }
    }

    let src_spec_payloads: Vec<Vec<Vec<f64>>> = spectra(src)
        .iter()
        .map(|s| {
            spectrum_arrays(s)
                .iter()
                .filter_map(|a| a.binary.as_ref())
                .map(binary_to_f64_vec)
                .collect::<Vec<_>>()
        })
        .collect();
    let modified_spec_payloads: Vec<Vec<Vec<f64>>> = spectra(&modified)
        .iter()
        .map(|s| {
            spectrum_arrays(s)
                .iter()
                .filter_map(|a| a.binary.as_ref())
                .map(binary_to_f64_vec)
                .collect::<Vec<_>>()
        })
        .collect();

    let src_chrom_payloads: Vec<Vec<Vec<f64>>> = chromatograms(src)
        .iter()
        .map(|c| {
            chromatogram_arrays(c)
                .iter()
                .filter_map(|a| a.binary.as_ref())
                .map(binary_to_f64_vec)
                .collect::<Vec<_>>()
        })
        .collect();
    let modified_chrom_payloads: Vec<Vec<Vec<f64>>> = chromatograms(&modified)
        .iter()
        .map(|c| {
            chromatogram_arrays(c)
                .iter()
                .filter_map(|a| a.binary.as_ref())
                .map(binary_to_f64_vec)
                .collect::<Vec<_>>()
        })
        .collect();

    assert_eq!(src_spec_payloads, modified_spec_payloads);
    assert_eq!(src_chrom_payloads, modified_chrom_payloads);
}

#[test]
fn pwiz_msdata_mzml_invariants_default_array_length_matches_payload() {
    let mzml = tiny_pwiz_11();

    for s in spectra(mzml) {
        let arrays = spectrum_arrays(s);
        if arrays.is_empty() {
            continue;
        }

        let first_len = arrays
            .iter()
            .filter_map(|a| a.binary.as_ref())
            .map(|b| match b {
                BinaryData::F64(v) => v.len(),
                BinaryData::F32(v) => v.len(),
                BinaryData::F16(v) => v.len(),
                BinaryData::I64(v) => v.len(),
                BinaryData::I32(v) => v.len(),
                BinaryData::I16(v) => v.len(),
            })
            .next();

        if let (Some(expected), Some(al)) = (first_len, s.default_array_length) {
            assert_eq!(expected, al, "spectrum {} array length mismatch", s.id);
        }
    }

    for c in chromatograms(mzml) {
        let arrays = chromatogram_arrays(c);
        if arrays.is_empty() {
            continue;
        }

        let first_len = arrays
            .iter()
            .filter_map(|a| a.binary.as_ref())
            .map(|b| match b {
                BinaryData::F64(v) => v.len(),
                BinaryData::F32(v) => v.len(),
                BinaryData::F16(v) => v.len(),
                BinaryData::I64(v) => v.len(),
                BinaryData::I32(v) => v.len(),
                BinaryData::I16(v) => v.len(),
            })
            .next();

        if let (Some(expected), Some(al)) = (first_len, c.default_array_length) {
            assert_eq!(expected, al, "chromatogram {} array length mismatch", c.id);
        }
    }
}

#[test]
fn pwiz_mzml_invariants_declared_counts_are_consistent_for_core_fixtures() {
    assert_declared_counts_consistent(tiny_pwiz_10());
    assert_declared_counts_consistent(tiny_pwiz_11());
    assert_declared_counts_consistent(small_pwiz_11());
}

#[test]
fn pwiz_mzml_invariants_declared_counts_are_consistent_after_b000_roundtrip() {
    let out = decode(&encode_bytes(tiny_pwiz_11(), 12, false)).expect("decode should succeed");
    assert_declared_counts_consistent(&out);
}

#[test]
fn pwiz_references_mzml_all_internal_refs_are_resolved_tiny_10() {
    assert_all_refs_resolved(tiny_pwiz_10());
}

#[test]
fn pwiz_references_mzml_all_internal_refs_are_resolved_tiny_11() {
    assert_all_refs_resolved(tiny_pwiz_11());
}

#[test]
fn pwiz_references_mzml_all_internal_refs_are_resolved_small_11() {
    assert_all_refs_resolved(small_pwiz_11());
}

#[test]
fn pwiz_references_mzml_detects_broken_run_default_source_file_ref() {
    let mut m = tiny_pwiz_11().clone();
    m.run.default_source_file_ref = Some("missing-source-file-id".to_string());

    let outcome = std::panic::catch_unwind(|| assert_all_refs_resolved(&m));
    assert!(
        outcome.is_err(),
        "broken defaultSourceFileRef should be detected"
    );
}

#[test]
fn pwiz_references_mzml_detects_broken_precursor_spectrum_ref() {
    let mut m = tiny_pwiz_11().clone();
    let s20 = m
        .run
        .spectrum_list
        .as_mut()
        .expect("spectrumList")
        .spectra
        .iter_mut()
        .find(|s| s.id == "scan=20")
        .expect("scan=20");
    let precursor = precursor_list_of_spectrum(s20)
        .and_then(|pl| pl.precursors.first())
        .cloned()
        .expect("scan=20 precursor");

    if let Some(pl) = s20.precursor_list.as_mut() {
        pl.precursors[0] = Precursor {
            spectrum_ref: Some("scan=DOES_NOT_EXIST".to_string()),
            ..precursor
        };
    }

    let outcome = std::panic::catch_unwind(|| assert_all_refs_resolved(&m));
    assert!(
        outcome.is_err(),
        "broken precursor.spectrumRef should be detected"
    );
}

#[test]
fn pwiz_spectrum_info_mzml_tiny_11_known_values_scan_19() {
    let mzml = tiny_pwiz_11();
    let s = spectrum_by_id(mzml, "scan=19");

    assert_eq!(s.ms_level, Some(1));
    assert_eq!(s.default_array_length, Some(15));

    let arrays = spectrum_arrays(s);
    let mz = find_array_by_accession(arrays, "MS:1000514");
    let intensity = find_array_by_accession(arrays, "MS:1000515");

    let mz_first = first_f64_from_binary(mz.binary.as_ref().expect("mz binary present")).unwrap();
    let i_first =
        first_f64_from_binary(intensity.binary.as_ref().expect("intensity binary present"))
            .unwrap();

    rel_close_f64(mz_first, 0.0, EPS_REL_F64, "scan=19 first m/z");
    rel_close_f64(i_first, 15.0, EPS_REL_F64, "scan=19 first intensity");
}

#[test]
fn pwiz_spectrum_info_mzml_tiny_11_known_values_scan_20() {
    let mzml = tiny_pwiz_11();
    let s = spectrum_by_id(mzml, "scan=20");

    assert_eq!(s.ms_level, Some(2));
    assert_eq!(s.default_array_length, Some(10));

    let arrays = spectrum_arrays(s);
    let mz = find_array_by_accession(arrays, "MS:1000514");
    let intensity = find_array_by_accession(arrays, "MS:1000515");

    let mz_first = first_f64_from_binary(mz.binary.as_ref().expect("mz binary present")).unwrap();
    let i_first =
        first_f64_from_binary(intensity.binary.as_ref().expect("intensity binary present"))
            .unwrap();

    rel_close_f64(mz_first, 0.0, EPS_REL_F64, "scan=20 first m/z");
    rel_close_f64(i_first, 20.0, EPS_REL_F64, "scan=20 first intensity");
}

#[test]
fn pwiz_spectrum_info_mzml_tiny_11_scan19_metadata_equivalence() {
    let s19 = spectrum_by_id(tiny_pwiz_11(), "scan=19");

    assert_eq!(parse_scan_number_from_id(&s19.id), Some(19));
    assert_eq!(s19.ms_level, Some(1));

    let rt_s = scan_start_time_seconds(s19).expect("scan start time");
    let mz_low = cv_value_f64(&s19.cv_params, "MS:1000528").expect("lowest observed m/z");
    let mz_high = cv_value_f64(&s19.cv_params, "MS:1000527").expect("highest observed m/z");

    rel_close_f64(rt_s, 353.43, 1e-6, "scan=19 RT (seconds)");
    rel_close_f64(mz_low, 400.39, 1e-6, "scan=19 mzLow");
    rel_close_f64(mz_high, 1795.56, 1e-6, "scan=19 mzHigh");
}

#[test]
fn pwiz_spectrum_info_mzml_tiny_11_scan20_precursor_equivalence() {
    let s20 = spectrum_by_id(tiny_pwiz_11(), "scan=20");

    assert_eq!(parse_scan_number_from_id(&s20.id), Some(20));
    assert_eq!(s20.ms_level, Some(2));

    let precursor = precursor_list_of_spectrum(s20)
        .and_then(|pl| pl.precursors.first())
        .expect("scan=20 precursor");
    let selected_ion = precursor
        .selected_ion_list
        .as_ref()
        .and_then(|sil| sil.selected_ions.first())
        .expect("scan=20 selected ion");

    let mz = cv_value_f64(&selected_ion.cv_params, "MS:1000744").expect("selected ion m/z present");
    let intensity =
        cv_value_f64(&selected_ion.cv_params, "MS:1000042").expect("peak intensity present");
    let charge = cv_param_by_accession(&selected_ion.cv_params, "MS:1000041")
        .and_then(|p| p.value.as_deref())
        .and_then(|v| v.parse::<i32>().ok())
        .expect("charge state present");

    rel_close_f64(mz, 445.34, 1e-6, "scan=20 precursor m/z");
    rel_close_f64(intensity, 120053.0, 1e-9, "scan=20 precursor intensity");
    assert_eq!(charge, 2);
}

#[test]
fn pwiz_decode_rejects_invalid_signature() {
    let mut bytes = encode_bytes(tiny_pwiz_11(), 9, false);
    bytes[0] = b'X';

    let err = decode(&bytes).expect_err("decode must reject invalid signature");
    assert!(err.contains("file_signature") || err.contains("signature"));
}

#[test]
fn pwiz_decode_rejects_invalid_header_version_word() {
    let mut bytes = encode_bytes(tiny_pwiz_11(), 9, false);
    bytes[4..8].copy_from_slice(b"X999");
    let err = decode(&bytes).expect_err("decode must reject unsupported header version");
    assert!(
        err.contains("version") || err.contains("signature") || err.contains("endianness_flag"),
        "unexpected decode error: {err}"
    );
}

#[test]
fn pwiz_decode_rejects_truncated_payload() {
    let bytes = encode_bytes(tiny_pwiz_11(), 9, false);
    let truncated = &bytes[..bytes.len() / 2];

    let _ = decode(truncated).expect_err("decode must reject truncated payload");
}

#[test]
fn pwiz_decode_rejects_corrupted_offset_range() {
    let mut bytes = encode_bytes(tiny_pwiz_11(), 9, false);

    // corrupt `off_spec_entries` at header offset 8 with a very large value
    bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());

    let _ = decode(&bytes).expect_err("decode must reject invalid section offset");
}

#[test]
fn pwiz_binary_data_encoder_b000_roundtrip_preserves_source_file_ids() {
    let src = tiny_pwiz_11();
    let bytes = encode_bytes(src, 12, false);
    let out = decode(&bytes).expect("decode should succeed");

    assert_eq!(
        top_level_source_file_ids(src),
        top_level_source_file_ids(&out),
        "source file ids changed after b000 roundtrip"
    );
}

#[test]
fn pwiz_binary_data_encoder_b000_roundtrip_preserves_software_ids() {
    let src = tiny_pwiz_11();
    let bytes = encode_bytes(src, 12, false);
    let out = decode(&bytes).expect("decode should succeed");

    assert_eq!(
        top_level_software_ids(src),
        top_level_software_ids(&out),
        "software ids changed after b000 roundtrip"
    );
}

#[test]
fn pwiz_parse_and_roundtrip_parser_internal_fixture_regression_guard() {
    let src = parse_rel("crates/parser/data/mzml/tiny.pwiz.mzML0.99.10.mzML", false);
    let bytes = encode_bytes(&src, 12, false);
    let out = decode(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(&src, &out);
}

#[test]
fn pwiz_parse_and_roundtrip_parser_internal_fixture_anpc_regression_guard() {
    let src = anpc_test_mzml();
    let bytes = encode_bytes(src, 12, false);
    let out = decode(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(src, &out);
}

#[test]
fn pwiz_breaker_b000_roundtrip_scan_attributes_not_lost() {
    let mut src = tiny_pwiz_11().clone();

    let source_file_id = src
        .file_description
        .as_ref()
        .expect("fileDescription must exist")
        .source_file_list
        .source_file
        .first()
        .expect("sourceFile id must exist")
        .id
        .clone();
    let instrument_id = src
        .instrument_list
        .as_ref()
        .and_then(|il| il.instrument.first())
        .expect("instrument id must exist")
        .id
        .clone();
    let spectrum_ref_id = src
        .run
        .spectrum_list
        .as_ref()
        .and_then(|sl| sl.spectra.get(1))
        .expect("second spectrum must exist")
        .id
        .clone();

    let first_spectrum = src
        .run
        .spectrum_list
        .as_mut()
        .expect("spectrumList")
        .spectra
        .first_mut()
        .expect("first spectrum");
    let target_spectrum_id = first_spectrum.id.clone();
    let scan = first_scan_mut(first_spectrum);
    scan.instrument_configuration_ref = Some(instrument_id.clone());
    scan.source_file_ref = Some(source_file_id.clone());
    scan.external_spectrum_id = Some("external-scan-id:42".to_string());
    scan.spectrum_ref = Some(spectrum_ref_id.clone());

    let out = decode(&encode_bytes(&src, 12, false)).expect("decode should succeed");
    let out_first_spectrum = spectrum_by_id(&out, target_spectrum_id.as_str());
    let out_scan = first_scan(out_first_spectrum);

    assert_eq!(
        out_scan.instrument_configuration_ref.as_deref(),
        Some(instrument_id.as_str())
    );
    assert_eq!(
        out_scan.source_file_ref.as_deref(),
        Some(source_file_id.as_str())
    );
    assert_eq!(
        out_scan.external_spectrum_id.as_deref(),
        Some("external-scan-id:42")
    );
    assert_eq!(
        out_scan.spectrum_ref.as_deref(),
        Some(spectrum_ref_id.as_str())
    );
}

#[test]
fn pwiz_breaker_b000_roundtrip_spectrum_product_attributes_not_lost() {
    let mut src = tiny_pwiz_11().clone();

    let source_file_id = src
        .file_description
        .as_ref()
        .expect("fileDescription must exist")
        .source_file_list
        .source_file
        .first()
        .expect("sourceFile id must exist")
        .id
        .clone();
    let spectrum_ref_id = src
        .run
        .spectrum_list
        .as_ref()
        .and_then(|sl| sl.spectra.get(1))
        .expect("second spectrum must exist")
        .id
        .clone();

    let first_spectrum = src
        .run
        .spectrum_list
        .as_mut()
        .expect("spectrumList")
        .spectra
        .first_mut()
        .expect("first spectrum");
    let target_spectrum_id = first_spectrum.id.clone();
    let product = ensure_first_product_mut(first_spectrum);
    product.spectrum_ref = Some(spectrum_ref_id.clone());
    product.source_file_ref = Some(source_file_id.clone());
    product.external_spectrum_id = Some("external-product-id:7".to_string());

    let out = decode(&encode_bytes(&src, 12, false)).expect("decode should succeed");
    let out_first_spectrum = spectrum_by_id(&out, target_spectrum_id.as_str());
    let out_product = product_list_of_spectrum(out_first_spectrum)
        .and_then(|pl| pl.products.first())
        .unwrap_or_else(|| panic!("spectrum product was dropped in b000 roundtrip"));

    assert_eq!(
        out_product.spectrum_ref.as_deref(),
        Some(spectrum_ref_id.as_str())
    );
    assert_eq!(
        out_product.source_file_ref.as_deref(),
        Some(source_file_id.as_str())
    );
    assert_eq!(
        out_product.external_spectrum_id.as_deref(),
        Some("external-product-id:7")
    );
}

#[test]
fn pwiz_breaker_b000_roundtrip_chrom_product_attributes_not_lost() {
    let mut src = tiny_pwiz_11().clone();

    let source_file_id = src
        .file_description
        .as_ref()
        .expect("fileDescription must exist")
        .source_file_list
        .source_file
        .first()
        .expect("sourceFile id must exist")
        .id
        .clone();
    let spectrum_ref_id = src
        .run
        .spectrum_list
        .as_ref()
        .and_then(|sl| sl.spectra.first())
        .expect("first spectrum must exist")
        .id
        .clone();

    let first_chromatogram = src
        .run
        .chromatogram_list
        .as_mut()
        .expect("chromatogramList")
        .chromatograms
        .first_mut()
        .expect("first chromatogram");
    let target_chromatogram_id = first_chromatogram.id.clone();
    let product = first_chromatogram
        .product
        .get_or_insert_with(Product::default);
    product.spectrum_ref = Some(spectrum_ref_id.clone());
    product.source_file_ref = Some(source_file_id.clone());
    product.external_spectrum_id = Some("external-chrom-product-id:3".to_string());

    let out = decode(&encode_bytes(&src, 12, false)).expect("decode should succeed");
    let out_first_chromatogram = chromatogram_by_id(&out, target_chromatogram_id.as_str());
    let out_product = out_first_chromatogram
        .product
        .as_ref()
        .unwrap_or_else(|| panic!("chromatogram product was dropped in b000 roundtrip"));

    assert_eq!(
        out_product.spectrum_ref.as_deref(),
        Some(spectrum_ref_id.as_str())
    );
    assert_eq!(
        out_product.source_file_ref.as_deref(),
        Some(source_file_id.as_str())
    );
    assert_eq!(
        out_product.external_spectrum_id.as_deref(),
        Some("external-chrom-product-id:3")
    );
}

#[test]
fn pwiz_breaker_b000_roundtrip_scan_referenceable_param_group_refs_not_lost() {
    let mut src = tiny_pwiz_11().clone();
    let ref_group_id = "pwiz-breaker-scan-ref-group";
    ensure_referenceable_param_group(&mut src, ref_group_id);

    let first_spectrum = src
        .run
        .spectrum_list
        .as_mut()
        .expect("spectrumList")
        .spectra
        .first_mut()
        .expect("first spectrum");
    let target_spectrum_id = first_spectrum.id.clone();
    first_scan_mut(first_spectrum).referenceable_param_group_refs =
        vec![ReferenceableParamGroupRef {
            r#ref: ref_group_id.to_string(),
        }];

    let out = decode(&encode_bytes(&src, 12, false)).expect("decode should succeed");
    let out_first_spectrum = spectrum_by_id(&out, target_spectrum_id.as_str());

    let out_refs = &first_scan(out_first_spectrum).referenceable_param_group_refs;
    assert!(
        out_refs.iter().any(|r| r.r#ref == ref_group_id),
        "scan referenceableParamGroupRef lost in b000 roundtrip"
    );
}

#[test]
fn pwiz_breaker_b000_roundtrip_binary_data_array_refs_not_lost() {
    let mut src = tiny_pwiz_11().clone();
    let ref_group_id = "pwiz-breaker-bda-ref-group";
    ensure_referenceable_param_group(&mut src, ref_group_id);

    let first_spectrum = src
        .run
        .spectrum_list
        .as_mut()
        .expect("spectrumList")
        .spectra
        .first_mut()
        .expect("first spectrum");
    let target_spectrum_id = first_spectrum.id.clone();
    let first_array = first_spectrum
        .binary_data_array_list
        .as_mut()
        .and_then(|bal| bal.binary_data_arrays.first_mut())
        .expect("first spectrum binaryDataArray");
    first_array.referenceable_param_group_refs = vec![ReferenceableParamGroupRef {
        r#ref: ref_group_id.to_string(),
    }];

    let out = decode(&encode_bytes(&src, 12, false)).expect("decode should succeed");
    let out_first_spectrum = spectrum_by_id(&out, target_spectrum_id.as_str());
    let out_first_array = out_first_spectrum
        .binary_data_array_list
        .as_ref()
        .and_then(|bal| bal.binary_data_arrays.first())
        .expect("first spectrum binaryDataArray");

    assert!(
        out_first_array
            .referenceable_param_group_refs
            .iter()
            .any(|r| r.r#ref == ref_group_id),
        "binaryDataArray referenceableParamGroupRef lost in b000 roundtrip"
    );
}

#[test]
fn pwiz_mzml_fixture_presence_guard() {
    let must_exist = [
        "pwiz/example_data/tiny.pwiz.1.0.mzML",
        "pwiz/example_data/tiny.pwiz.1.1.mzML",
        "pwiz/example_data/small.pwiz.1.0.mzML",
        "pwiz/example_data/small.pwiz.1.1.mzML",
        "pwiz/pwiz/data/msdata/BinaryDataEncoderTest.cpp",
        "pwiz/pwiz/data/msdata/IOTest.cpp",
        "pwiz/pwiz/data/msdata/MSDataFileTest.cpp",
        "pwiz/pwiz/data/msdata/ReaderTest.cpp",
        "pwiz/pwiz/data/msdata/DiffTest.cpp",
        "pwiz/pwiz/data/msdata/ReferencesTest.cpp",
        "pwiz/pwiz/data/msdata/SpectrumInfoTest.cpp",
        "pwiz/pwiz/data/msdata/Serializer_mzML_Test.cpp",
        "pwiz/pwiz/data/msdata/SpectrumList_mzML_Test.cpp",
        "pwiz/pwiz/data/msdata/ChromatogramList_mzML_Test.cpp",
    ];

    for rel in must_exist {
        let p = repo_root().join(rel);
        assert!(p.exists(), "required pwiz fixture missing: {}", p.display());
    }
}
