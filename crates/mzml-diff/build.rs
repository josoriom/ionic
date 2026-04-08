/// build.rs for mzml-diff
///
/// Reads the two derived TOML artefacts:
///   crates/mzml-diff/spec-derived/mzml-schema.toml
///   crates/mzml-diff/spec-derived/psi-ms-cv.toml
///
/// Emits:  $OUT_DIR/semantic_tables.rs
///
/// The generated file contains:
///   - CV_VALUE_TYPE_KEYS / CV_VALUE_TYPE_VALS: sorted parallel arrays for
///     binary-search lookup of CV accession → value-type encoding
///     (0=none/string, 1=xsd:float, 2=xsd:int, 3=xsd:string)
///   - cv_value_type():  convenience lookup function
///   - IDREF_ATTRS:      &[&str] of attribute names that are cross-references
///   - COUNTER_ATTRS:    &[&str] (reference only — not used at runtime)
///   - REQUIRED_ELEMENTS / OPTIONAL_ELEMENTS: &[&str] from XSD minOccurs
///
/// Note: TRANSPORT_ATTRS and TRANSPORT_ELEMENTS are hand-curated in
/// `src/tables.rs` (not generated) because the XSD/OBO specs don't classify
/// which attributes/elements are transport vs semantic.
use std::{collections::BTreeMap, env, fs, path::PathBuf};

use serde::Deserialize;

// ── TOML schemas ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SchemaMeta {
    #[allow(dead_code)]
    source: String,
    #[allow(dead_code)]
    version: String,
}

#[derive(Deserialize)]
struct XsdTypeToml {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    extends: Option<String>,
    #[serde(default)]
    elements: Vec<XsdElementToml>,
    #[serde(default)]
    attributes: Vec<XsdAttrToml>,
}

#[derive(Deserialize)]
struct XsdElementToml {
    name: String,
    min_occurs: u32,
    #[allow(dead_code)]
    type_ref: String,
}

#[derive(Deserialize)]
struct XsdAttrToml {
    name: String,
    #[allow(dead_code)]
    required: bool,
    type_ref: String,
}

#[derive(Deserialize)]
struct MzmlSchemaToml {
    #[allow(dead_code)]
    meta: SchemaMeta,
    #[serde(default)]
    types: Vec<XsdTypeToml>,
}

#[derive(Deserialize)]
struct CvMeta {
    #[allow(dead_code)]
    source: String,
    #[allow(dead_code)]
    version: String,
}

#[derive(Deserialize)]
struct CvTermToml {
    id: String,
    #[allow(dead_code)]
    name: String,
    value_type: String,
    is_obsolete: bool,
}

#[derive(Deserialize)]
struct PsiMsCvToml {
    #[allow(dead_code)]
    meta: CvMeta,
    #[serde(default)]
    terms: Vec<CvTermToml>,
}

// ── code-gen helpers ─────────────────────────────────────────────────────────

fn emit_str_array(out: &mut String, name: &str, items: &[String]) {
    out.push_str(&format!("pub static {name}: &[&str] = &[\n"));
    for item in items {
        out.push_str(&format!("    {:?},\n", item));
    }
    out.push_str("];\n\n");
}

/// Emit a sorted parallel pair of arrays suitable for binary search:
///   KEYS_<name>: &[&str]  (sorted accessions / keys)
///   VALS_<name>: &[u8]    (parallel values encoded as u8)
///
/// value_type encoding:
///   0 = none / string-or-absent
///   1 = xsd:float
///   2 = xsd:int
///   3 = xsd:string
fn encode_vtype(v: &str) -> u8 {
    match v {
        "xsd:float" | "xsd:double" | "xsd:decimal" => 1,
        "xsd:int" | "xsd:integer" | "xsd:nonNegativeInteger" | "xsd:positiveInteger" => 2,
        "xsd:string" => 3,
        _ => 0,
    }
}

fn emit_cv_tables(out: &mut String, entries: &BTreeMap<String, u8>) {
    let keys: Vec<&String> = entries.keys().collect();
    let vals: Vec<u8> = entries.values().copied().collect();

    out.push_str("/// Sorted accession keys for CV value-type lookup.\n");
    out.push_str("/// Use binary search against CV_VALUE_TYPE_VALS for the type.\n");
    out.push_str("/// 0=none/string, 1=xsd:float, 2=xsd:int, 3=xsd:string\n");
    out.push_str("pub static CV_VALUE_TYPE_KEYS: &[&str] = &[\n");
    for k in &keys {
        out.push_str(&format!("    {:?},\n", k));
    }
    out.push_str("];\n\n");

    out.push_str("pub static CV_VALUE_TYPE_VALS: &[u8] = &[\n");
    for v in &vals {
        out.push_str(&format!("    {v},\n"));
    }
    out.push_str("];\n\n");

    // Convenience lookup fn
    out.push_str(
        r#"/// Look up the value type for a CV accession.
/// Returns 0 (none/string) for unknown accessions.
#[inline]
pub fn cv_value_type(accession: &str) -> u8 {
    match CV_VALUE_TYPE_KEYS.binary_search(&accession) {
        Ok(idx) => CV_VALUE_TYPE_VALS[idx],
        Err(_) => 0,
    }
}

"#,
    );
}

// ── main ─────────────────────────────────────────────────────────────────────

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let schema_path = manifest.join("spec-derived/mzml-schema.toml");
    let cv_path = manifest.join("spec-derived/psi-ms-cv.toml");

    // Tell Cargo to re-run if the TOMLs change.
    println!("cargo:rerun-if-changed={}", schema_path.display());
    println!("cargo:rerun-if-changed={}", cv_path.display());

    // ── Read TOMLs ────────────────────────────────────────────────────────

    let schema_text = fs::read_to_string(&schema_path)
        .unwrap_or_else(|e| panic!("Cannot read {}: {e}", schema_path.display()));
    let cv_text = fs::read_to_string(&cv_path)
        .unwrap_or_else(|e| panic!("Cannot read {}: {e}", cv_path.display()));

    let schema: MzmlSchemaToml = toml::from_str(&schema_text)
        .unwrap_or_else(|e| panic!("Cannot parse mzml-schema.toml: {e}"));
    let cv: PsiMsCvToml =
        toml::from_str(&cv_text).unwrap_or_else(|e| panic!("Cannot parse psi-ms-cv.toml: {e}"));

    // ── Derive tables from schema ─────────────────────────────────────────

    // IDREF attributes (cross-references that are transport, not content)
    let idref_attrs: Vec<String> = schema
        .types
        .iter()
        .flat_map(|t| t.attributes.iter())
        .filter(|a| a.type_ref == "IDREF")
        .map(|a| a.name.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    // BTreeSet already provides sorted + deduplicated output.

    // Counter attributes — emitted for reference/auditing only. The actual
    // transport-attr and transport-element lists used at runtime live in
    // `src/tables.rs` as hand-curated byte-string constants.
    let counter_attrs: Vec<String> = vec![
        "arrayLength".into(),
        "count".into(),
        "encodedLength".into(),
        "index".into(),
        "spotID".into(),
    ];

    // Required element names across all types (min_occurs >= 1)
    let required_elements: Vec<String> = schema
        .types
        .iter()
        .flat_map(|t| t.elements.iter())
        .filter(|e| e.min_occurs >= 1)
        .map(|e| e.name.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    // Optional element names (min_occurs == 0)
    let optional_elements: Vec<String> = schema
        .types
        .iter()
        .flat_map(|t| t.elements.iter())
        .filter(|e| e.min_occurs == 0)
        .map(|e| e.name.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    // ── Derive CV value-type table ────────────────────────────────────────

    let cv_value_types: BTreeMap<String, u8> = cv
        .terms
        .iter()
        .filter(|t| !t.is_obsolete && t.value_type != "none")
        .map(|t| (t.id.clone(), encode_vtype(&t.value_type)))
        .collect();

    // ── Emit Rust ─────────────────────────────────────────────────────────

    let mut out = String::new();
    out.push_str("// @generated by build.rs — do not edit by hand.\n");
    out.push_str("// Source: spec-derived/mzml-schema.toml + spec-derived/psi-ms-cv.toml\n\n");

    emit_cv_tables(&mut out, &cv_value_types);
    emit_str_array(&mut out, "IDREF_ATTRS", &idref_attrs);
    emit_str_array(&mut out, "COUNTER_ATTRS", &counter_attrs);
    emit_str_array(&mut out, "REQUIRED_ELEMENTS", &required_elements);
    emit_str_array(&mut out, "OPTIONAL_ELEMENTS", &optional_elements);

    let out_path = out_dir.join("semantic_tables.rs");
    fs::write(&out_path, &out)
        .unwrap_or_else(|e| panic!("Cannot write {}: {e}", out_path.display()));
}
