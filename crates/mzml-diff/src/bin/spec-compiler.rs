/// spec-compiler: derives mzml-schema.toml and psi-ms-cv.toml from the
/// canonical spec files (mzML1.1.1.xsd and psi-ms.obo).
///
/// Usage:
///   spec-compiler --xsd  spec-files/mzML1.1.1.xsd \
///                 --obo  spec-files/psi-ms.obo \
///                 --out  spec-derived/
///
/// The two output TOMLs are tracked in Git as inspectable artefacts.
/// build.rs then reads them at compile time and emits semantic_tables.rs.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

// ── CLI ──────────────────────────────────────────────────────────────────────

fn usage() -> ! {
    eprintln!(
        "Usage: spec-compiler \
         --xsd <mzML1.1.1.xsd> \
         --obo <psi-ms.obo> \
         --out <output-dir>"
    );
    std::process::exit(1);
}

struct Args {
    xsd: PathBuf,
    obo: PathBuf,
    out: PathBuf,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut xsd = None;
    let mut obo = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--xsd" => xsd = args.next().map(PathBuf::from),
            "--obo" => obo = args.next().map(PathBuf::from),
            "--out" => out = args.next().map(PathBuf::from),
            _ => {}
        }
    }
    Args {
        xsd: xsd.unwrap_or_else(|| usage()),
        obo: obo.unwrap_or_else(|| usage()),
        out: out.unwrap_or_else(|| usage()),
    }
}

// ── XSD parser ───────────────────────────────────────────────────────────────
//
// We only need a narrow slice of the XSD:
//   - complexType names  →  element children (name, minOccurs, type)
//                       →  attribute children (name, use, type)
//   - extension base     →  which type this one extends
//
// We use a simple state-machine over quick-xml events; no full schema
// validation needed.

#[derive(Debug, Default, Clone)]
struct XsdElement {
    name: String,
    min_occurs: u32, // 0 = optional, ≥1 = required
    type_ref: String,
}

#[derive(Debug, Default, Clone)]
struct XsdAttribute {
    name: String,
    required: bool,
    type_ref: String, // xs:string, xs:ID, xs:IDREF, xs:int, …
}

#[derive(Debug, Default, Clone)]
struct XsdType {
    name: String,
    extends: Option<String>, // xs:extension base
    elements: Vec<XsdElement>,
    attributes: Vec<XsdAttribute>,
}

/// Parse the XSD and return a map of type-name → XsdType.
fn parse_xsd(path: &Path) -> BTreeMap<String, XsdType> {
    use quick_xml::{Reader, events::Event};

    let text = fs::read_to_string(path).expect("cannot read XSD");
    let mut reader = Reader::from_str(&text);
    reader.config_mut().trim_text(true);

    let mut types: BTreeMap<String, XsdType> = BTreeMap::new();
    let mut current: Option<XsdType> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let qname = e.name();
                let local = local_name(qname.as_ref());

                match local {
                    "complexType" => {
                        let name = attr_value(e, b"name").unwrap_or_default();
                        let t = XsdType {
                            name,
                            ..Default::default()
                        };
                        current = Some(t);
                    }
                    "extension" if current.is_some() => {
                        if let Some(base) = attr_value(e, b"base") {
                            let base = strip_prefix(&base, "dx:");
                            if let Some(ref mut t) = current {
                                t.extends = Some(base);
                            }
                        }
                    }
                    "element" if current.is_some() => {
                        if let Some(name) = attr_value(e, b"name") {
                            let type_ref = attr_value(e, b"type")
                                .map(|s| strip_prefix(&s, "dx:"))
                                .unwrap_or_default();
                            let min_occurs = attr_value(e, b"minOccurs")
                                .and_then(|v| v.parse::<u32>().ok())
                                .unwrap_or(1);
                            if let Some(ref mut t) = current {
                                t.elements.push(XsdElement {
                                    name,
                                    min_occurs,
                                    type_ref,
                                });
                            }
                        }
                    }
                    "attribute" if current.is_some() => {
                        if let Some(name) = attr_value(e, b"name") {
                            let use_val =
                                attr_value(e, b"use").unwrap_or_else(|| "optional".into());
                            let type_ref = attr_value(e, b"type")
                                .map(|s| strip_prefix(&s, "xs:"))
                                .unwrap_or_else(|| "string".into());
                            if let Some(ref mut t) = current {
                                t.attributes.push(XsdAttribute {
                                    name,
                                    required: use_val == "required",
                                    type_ref,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let qname = e.name();
                let local = local_name(qname.as_ref());
                if local == "complexType"
                    && let Some(t) = current.take()
                    && !t.name.is_empty()
                {
                    types.insert(t.name.clone(), t);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => panic!("XSD parse error: {e}"),
            _ => {}
        }
        buf.clear();
    }

    types
}

fn local_name(qname: &[u8]) -> &str {
    let s = std::str::from_utf8(qname).unwrap_or("");
    if let Some(pos) = s.rfind(':') {
        &s[pos + 1..]
    } else {
        s
    }
}

fn attr_value(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| {
            std::str::from_utf8(a.value.as_ref())
                .ok()
                .map(|s| s.to_owned())
        })
}

fn strip_prefix(s: &str, prefix: &str) -> String {
    s.strip_prefix(prefix).unwrap_or(s).to_owned()
}

// ── OBO parser ────────────────────────────────────────────────────────────────
//
// We extract, for every [Term]:
//   id, name, has_value_type (xsd:float | xsd:int | xsd:string | …)
//
// Terms without has_value_type get value_type = "none".

#[derive(Debug, Clone)]
struct OboTerm {
    id: String,
    name: String,
    value_type: String, // "xsd:float" | "xsd:int" | "xsd:string" | "none"
    is_obsolete: bool,
}

fn parse_obo(path: &Path) -> Vec<OboTerm> {
    let file = fs::File::open(path).expect("cannot open OBO");
    let reader = BufReader::new(file);

    let mut terms: Vec<OboTerm> = Vec::new();
    let mut in_term = false;
    let mut cur_id = String::new();
    let mut cur_name = String::new();
    let mut cur_vtype = String::new();
    let mut cur_obsolete = false;

    /// Flush the current term accumulator into the output vec, if valid.
    fn flush_term(
        terms: &mut Vec<OboTerm>,
        id: &mut String,
        name: &mut String,
        vtype: &mut String,
        obsolete: &mut bool,
    ) {
        if !id.is_empty() {
            terms.push(OboTerm {
                id: std::mem::take(id),
                name: std::mem::take(name),
                value_type: if vtype.is_empty() {
                    "none".into()
                } else {
                    std::mem::take(vtype)
                },
                is_obsolete: *obsolete,
            });
        }
        id.clear();
        name.clear();
        vtype.clear();
        *obsolete = false;
    }

    for line_res in reader.lines() {
        let line = line_res.expect("IO error reading OBO");
        let line = line.trim();

        if line == "[Term]" {
            if in_term {
                flush_term(&mut terms, &mut cur_id, &mut cur_name, &mut cur_vtype, &mut cur_obsolete);
            }
            in_term = true;
            continue;
        }

        if line.starts_with('[') {
            if in_term {
                flush_term(&mut terms, &mut cur_id, &mut cur_name, &mut cur_vtype, &mut cur_obsolete);
            }
            in_term = false;
            continue;
        }

        if !in_term {
            continue;
        }

        if let Some(val) = strip_tag(line, "id: ") {
            cur_id = val.to_owned();
        } else if let Some(val) = strip_tag(line, "name: ") {
            cur_name = val.to_owned();
        } else if line == "is_obsolete: true" {
            cur_obsolete = true;
        } else if let Some(val) = strip_tag(line, "relationship: has_value_type ") {
            // val might be "xsd:float ! The allowed value-type..." — take first token
            cur_vtype = val.split_whitespace().next().unwrap_or("").to_owned();
        }
    }

    // Flush last term.
    if in_term {
        flush_term(&mut terms, &mut cur_id, &mut cur_name, &mut cur_vtype, &mut cur_obsolete);
    }

    terms
}

fn strip_tag<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.strip_prefix(prefix)
}

// ── TOML serialisers ─────────────────────────────────────────────────────────
//
// We hand-roll the TOML output — no serde dependency needed at runtime for
// this simple tool, keeping it lean.

/// Write mzml-schema.toml
///
/// Format:
///
/// [meta]
/// source = "mzML1.1.1.xsd"
/// version = "1.1.1"
///
/// [[types]]
/// name = "SpectrumType"
/// extends = "ParamGroupType"   # optional
///
/// [[types.elements]]
/// name = "scanList"
/// min_occurs = 0
/// type_ref = "ScanListType"
///
/// [[types.attributes]]
/// name = "id"
/// required = true
/// type_ref = "string"
fn write_mzml_schema_toml(types: &BTreeMap<String, XsdType>, out: &Path) {
    let mut s = String::new();
    s.push_str("[meta]\n");
    s.push_str("source = \"mzML1.1.1.xsd\"\n");
    s.push_str("version = \"1.1.1\"\n");
    s.push('\n');

    for t in types.values() {
        s.push_str("[[types]]\n");
        s.push_str(&format!("name = {:?}\n", t.name));
        if let Some(ref base) = t.extends {
            s.push_str(&format!("extends = {:?}\n", base));
        }
        s.push('\n');

        for el in &t.elements {
            s.push_str("[[types.elements]]\n");
            s.push_str(&format!("name = {:?}\n", el.name));
            s.push_str(&format!("min_occurs = {}\n", el.min_occurs));
            s.push_str(&format!("type_ref = {:?}\n", el.type_ref));
            s.push('\n');
        }

        for attr in &t.attributes {
            s.push_str("[[types.attributes]]\n");
            s.push_str(&format!("name = {:?}\n", attr.name));
            s.push_str(&format!("required = {}\n", attr.required));
            s.push_str(&format!("type_ref = {:?}\n", attr.type_ref));
            s.push('\n');
        }
    }

    fs::write(out, s).expect("cannot write mzml-schema.toml");
}

/// Write psi-ms-cv.toml
///
/// Format:
///
/// [meta]
/// source = "psi-ms.obo"
/// version = "4.1.244"
///
/// [[terms]]
/// id = "MS:1000514"
/// name = "m/z array"
/// value_type = "xsd:float"
/// is_obsolete = false
fn write_psi_ms_cv_toml(terms: &[OboTerm], obo_version: &str, out: &Path) {
    let mut s = String::new();
    s.push_str("[meta]\n");
    s.push_str("source = \"psi-ms.obo\"\n");
    s.push_str(&format!("version = {:?}\n", obo_version));
    s.push('\n');

    for t in terms {
        s.push_str("[[terms]]\n");
        s.push_str(&format!("id = {:?}\n", t.id));
        s.push_str(&format!("name = {:?}\n", t.name));
        s.push_str(&format!("value_type = {:?}\n", t.value_type));
        s.push_str(&format!("is_obsolete = {}\n", t.is_obsolete));
        s.push('\n');
    }

    fs::write(out, s).expect("cannot write psi-ms-cv.toml");
}

// ── Extract OBO version ───────────────────────────────────────────────────────

fn obo_version(path: &Path) -> String {
    let file = fs::File::open(path).expect("cannot open OBO for version");
    let reader = BufReader::new(file);
    for line_res in reader.lines() {
        let line = line_res.expect("IO");
        if let Some(v) = line.trim().strip_prefix("data-version: ") {
            return v.trim().to_owned();
        }
        // Stop after the header block
        if line.trim().starts_with('[') {
            break;
        }
    }
    "unknown".into()
}

// ── main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args = parse_args();

    // ── XSD → mzml-schema.toml ────────────────────────────────────────────
    eprintln!("Parsing XSD: {}", args.xsd.display());
    let types = parse_xsd(&args.xsd);
    eprintln!("  → {} complex types extracted", types.len());

    let schema_out = args.out.join("mzml-schema.toml");
    write_mzml_schema_toml(&types, &schema_out);
    eprintln!("  → written: {}", schema_out.display());

    // ── OBO → psi-ms-cv.toml ─────────────────────────────────────────────
    eprintln!("Parsing OBO: {}", args.obo.display());
    let version = obo_version(&args.obo);
    let terms = parse_obo(&args.obo);
    let total = terms.len();
    let with_vtype = terms.iter().filter(|t| t.value_type != "none").count();
    eprintln!(
        "  → {} terms extracted ({} with has_value_type), version {}",
        total, with_vtype, version
    );

    let cv_out = args.out.join("psi-ms-cv.toml");
    write_psi_ms_cv_toml(&terms, &version, &cv_out);
    eprintln!("  → written: {}", cv_out.display());

    // ── summary ───────────────────────────────────────────────────────────
    let float_count = terms.iter().filter(|t| t.value_type == "xsd:float").count();
    let int_count = terms.iter().filter(|t| t.value_type == "xsd:int").count();
    let string_count = terms
        .iter()
        .filter(|t| t.value_type == "xsd:string")
        .count();
    eprintln!("  value types: float={float_count}, int={int_count}, string={string_count}");

    // Element/attribute summary from XSD
    let required_attrs: usize = types
        .values()
        .flat_map(|t| t.attributes.iter())
        .filter(|a| a.required)
        .count();
    let optional_attrs: usize = types
        .values()
        .flat_map(|t| t.attributes.iter())
        .filter(|a| !a.required)
        .count();
    let required_elems: usize = types
        .values()
        .flat_map(|t| t.elements.iter())
        .filter(|e| e.min_occurs >= 1)
        .count();
    let optional_elems: usize = types
        .values()
        .flat_map(|t| t.elements.iter())
        .filter(|e| e.min_occurs == 0)
        .count();
    eprintln!(
        "XSD: attrs required={required_attrs} optional={optional_attrs}, \
         elements required={required_elems} optional={optional_elems}"
    );

    // Collect the set of attribute names that are IDREF (cross-references)
    let idref_attrs: BTreeSet<String> = types
        .values()
        .flat_map(|t| t.attributes.iter())
        .filter(|a| a.type_ref == "IDREF")
        .map(|a| a.name.clone())
        .collect();
    eprintln!("IDREF attributes: {idref_attrs:?}");
}
