use std::{borrow::Cow, io::BufRead, str::from_utf8};

use quick_xml::{
    XmlVersion,
    events::{BytesStart, Event},
};

use crate::{
    cv_table,
    ion::attr_meta::{borrow_or_own, borrow_prefix},
    mzml::{
        schema::TagId,
        structs::{
            CvEntry, CvParam, ReferenceableParamGroupRef, SoftwareParam, SourceFileRef, UserParam,
        },
        utilities::{ParseError, ParsingWorkspace, normalize_tag},
    },
};

#[inline]
pub fn xml_local_name(mut raw: &[u8]) -> &[u8] {
    if raw.first() == Some(&b'{')
        && let Some(end) = raw.iter().position(|&b| b == b'}')
    {
        raw = &raw[end + 1..];
    }
    if let Some(colon) = raw.iter().rposition(|&b| b == b':') {
        &raw[colon + 1..]
    } else {
        raw
    }
}

#[inline]
pub fn tag_id_from_bytes(raw: &[u8]) -> TagId {
    let local = from_utf8(xml_local_name(raw)).unwrap_or("");
    TagId::from_xml_tag(normalize_tag(local))
}

pub fn drain_until_close<R: BufRead>(
    ws: &mut ParsingWorkspace<R>,
    closing_bytes: &[u8],
) -> Result<(), ParseError> {
    let mut depth = 1usize;
    loop {
        match ws.next_event()? {
            Event::Start(_) => depth += 1,
            Event::End(e) => {
                depth -= 1;
                if depth == 0 && e.name().as_ref() == closing_bytes {
                    break Ok(());
                }
            }
            Event::Eof => break Ok(()),
            _ => {}
        }
    }
}

pub fn read_element_text<R: BufRead>(
    ws: &mut ParsingWorkspace<R>,
    closing_bytes: &[u8],
) -> Result<String, ParseError> {
    let mut text = String::new();
    loop {
        match ws.next_event()? {
            Event::Text(t) => text.push_str(&t.decode().map_err(quick_xml::Error::from)?),
            Event::CData(t) => text.push_str(&String::from_utf8_lossy(&t.into_inner())),
            Event::End(e) if e.name().as_ref() == closing_bytes => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(text)
}

pub fn read_base64_binary<R: BufRead>(
    ws: &mut ParsingWorkspace<R>,
    closing_bytes: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), ParseError> {
    out.clear();
    loop {
        match ws.next_event()? {
            Event::Text(t) => out.extend(
                t.as_ref()
                    .iter()
                    .copied()
                    .filter(|b| !b.is_ascii_whitespace()),
            ),
            Event::CData(t) => out.extend(
                t.into_inner()
                    .iter()
                    .copied()
                    .filter(|b| !b.is_ascii_whitespace()),
            ),
            Event::End(e) if e.name().as_ref() == closing_bytes => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

pub fn attr_any(element: &BytesStart, candidate_names: &[&[u8]]) -> Option<String> {
    for a in element.attributes().with_checks(false).flatten() {
        if candidate_names.iter().any(|n| *n == a.key.as_ref()) {
            return Some(
                a.normalized_value(XmlVersion::Implicit1_0)
                    .ok()?
                    .to_string(),
            );
        }
    }
    None
}

#[inline]
pub fn attr(e: &BytesStart, name: &[u8]) -> Option<String> {
    attr_any(e, &[name])
}

pub fn attr_str<'a>(element: &'a BytesStart, name: &[u8]) -> Option<Cow<'a, str>> {
    for a in element.attributes().with_checks(false).flatten() {
        if a.key.as_ref() == name {
            return a.normalized_value(XmlVersion::Implicit1_0).ok();
        }
    }
    None
}

#[inline]
pub fn attr_u32(e: &BytesStart, name: &[u8]) -> Option<u32> {
    attr_str(e, name).and_then(|s| s.parse().ok())
}
#[inline]
pub fn attr_usize(e: &BytesStart, name: &[u8]) -> Option<usize> {
    attr_str(e, name).and_then(|s| s.parse().ok())
}

pub const PREALLOC_CAP: usize = 1 << 16;

#[derive(Default)]
struct CvParamText<'a> {
    cv_ref: Option<Cow<'a, str>>,
    accession: Option<Cow<'a, str>>,
    name: Option<Cow<'a, str>>,
    value: Option<Cow<'a, str>>,
    unit_cv_ref: Option<Cow<'a, str>>,
    unit_name: Option<Cow<'a, str>>,
    unit_accession: Option<Cow<'a, str>>,
}

fn read_cv_param_text<'a>(element: &'a BytesStart) -> CvParamText<'a> {
    let mut found = CvParamText::default();

    for attribute in element.attributes().with_checks(false).flatten() {
        let Ok(text) = attribute.normalized_value(XmlVersion::Implicit1_0) else {
            continue;
        };
        match attribute.key.as_ref() {
            b"cvRef" | b"cvLabel" => found.cv_ref = Some(text),
            b"accession" => found.accession = Some(text),
            b"name" => found.name = Some(text),
            b"value" => found.value = Some(text),
            b"unitCvRef" | b"unitCvLabel" => found.unit_cv_ref = Some(text),
            b"unitName" => found.unit_name = Some(text),
            b"unitAccession" => found.unit_accession = Some(text),
            _ => {}
        }
    }

    found
}

#[inline]
pub fn read_cv_param(e: &BytesStart) -> CvParam {
    let found = read_cv_param_text(e);
    let term = found.accession.as_deref().and_then(cv_table::find_term);
    let unit_term = found
        .unit_accession
        .as_deref()
        .and_then(cv_table::find_term);

    CvParam {
        cv_ref: found.cv_ref.map(|text| borrow_prefix(&text)),
        accession: found
            .accession
            .map(|text| borrow_or_own(text, term.map(|term| term.accession))),
        name: found
            .name
            .map(|text| borrow_or_own(text, term.map(|term| term.name)))
            .unwrap_or(Cow::Borrowed("")),
        value: found.value.map(|text| Cow::Owned(text.into_owned())),
        unit_cv_ref: found.unit_cv_ref.map(|text| borrow_prefix(&text)),
        unit_name: found
            .unit_name
            .map(|text| borrow_or_own(text, unit_term.map(|term| term.name))),
        unit_accession: found
            .unit_accession
            .map(|text| borrow_or_own(text, unit_term.map(|term| term.accession))),
    }
}

#[inline]
pub fn read_user_param(e: &BytesStart) -> UserParam {
    UserParam {
        name: attr(e, b"name").unwrap_or_default(),
        r#type: attr(e, b"type"),
        unit_accession: attr(e, b"unitAccession"),
        unit_cv_ref: attr_any(e, &[b"unitCvRef", b"unitCvLabel"]),
        unit_name: attr(e, b"unitName"),
        value: attr(e, b"value"),
    }
}

#[inline]
pub fn read_software_param(e: &BytesStart) -> SoftwareParam {
    SoftwareParam {
        cv_ref: attr_any(e, &[b"cvRef", b"cvLabel"]),
        accession: attr(e, b"accession").unwrap_or_default(),
        name: attr(e, b"name").unwrap_or_default(),
        version: attr(e, b"version"),
    }
}

#[inline]
pub fn read_ref_group_ref(e: &BytesStart) -> ReferenceableParamGroupRef {
    ReferenceableParamGroupRef {
        r#ref: attr(e, b"ref").unwrap_or_default(),
    }
}
#[inline]
pub fn read_source_file_ref(e: &BytesStart) -> SourceFileRef {
    SourceFileRef {
        r#ref: attr(e, b"ref").unwrap_or_default(),
    }
}
#[inline]
pub fn read_cv_entry(e: &BytesStart) -> CvEntry {
    CvEntry {
        id: attr_any(e, &[b"id", b"cvLabel"]).unwrap_or_default(),
        full_name: attr(e, b"fullName"),
        version: attr(e, b"version"),
        uri: attr(e, b"URI"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_param(xml: &str) -> CvParam {
        let element = BytesStart::from_content(&xml[1..xml.len() - 2], "cvParam".len());
        read_cv_param(&element)
    }

    #[test]
    fn read_cv_param_gets_every_attribute() {
        let param = read_param(
            r#"<cvParam cvRef="MS" accession="MS:1000016" name="scan start time" value="12.5" unitCvRef="UO" unitAccession="UO:0000031" unitName="minute"/>"#,
        );
        assert_eq!(param.cv_ref.as_deref(), Some("MS"));
        assert_eq!(param.accession.as_deref(), Some("MS:1000016"));
        assert_eq!(param.name, "scan start time");
        assert_eq!(param.value.as_deref(), Some("12.5"));
        assert_eq!(param.unit_cv_ref.as_deref(), Some("UO"));
        assert_eq!(param.unit_accession.as_deref(), Some("UO:0000031"));
        assert_eq!(param.unit_name.as_deref(), Some("minute"));
    }

    #[test]
    fn read_cv_param_allows_the_label_spelling() {
        let param = read_param(
            r#"<cvParam cvLabel="MS" accession="MS:1000511" name="ms level" value="1" unitCvLabel="UO"/>"#,
        );
        assert_eq!(param.cv_ref.as_deref(), Some("MS"));
        assert_eq!(param.unit_cv_ref.as_deref(), Some("UO"));
    }

    #[test]
    fn read_cv_param_allows_any_attribute_order() {
        let param =
            read_param(r#"<cvParam value="1" name="ms level" accession="MS:1000511" cvRef="MS"/>"#);
        assert_eq!(param.accession.as_deref(), Some("MS:1000511"));
        assert_eq!(param.name, "ms level");
        assert_eq!(param.value.as_deref(), Some("1"));
    }

    #[test]
    fn read_cv_param_borrows_known_text() {
        let param = read_param(
            r#"<cvParam cvRef="MS" accession="MS:1000511" name="ms level" value="1" unitCvRef="UO" unitAccession="UO:0000031" unitName="minute"/>"#,
        );
        assert!(matches!(param.cv_ref, Some(Cow::Borrowed(_))));
        assert!(matches!(param.accession, Some(Cow::Borrowed(_))));
        assert!(matches!(param.name, Cow::Borrowed(_)));
        assert!(matches!(param.unit_cv_ref, Some(Cow::Borrowed(_))));
        assert!(matches!(param.unit_accession, Some(Cow::Borrowed(_))));
        assert!(matches!(param.unit_name, Some(Cow::Borrowed(_))));
    }

    #[test]
    fn read_cv_param_owns_the_value() {
        let param =
            read_param(r#"<cvParam cvRef="MS" accession="MS:1000511" name="ms level" value="1"/>"#);
        assert!(matches!(param.value, Some(Cow::Owned(_))));
    }

    #[test]
    fn read_cv_param_keeps_a_name_the_table_does_not_have() {
        let param =
            read_param(r#"<cvParam cvRef="MS" accession="MS:1000511" name="vendor level text"/>"#);
        assert_eq!(param.name, "vendor level text");
        assert!(matches!(param.name, Cow::Owned(_)));
    }

    #[test]
    fn read_cv_param_keeps_an_accession_the_table_does_not_have() {
        let param =
            read_param(r#"<cvParam cvRef="MS" accession="MS:9999999" name="vendor term"/>"#);
        assert_eq!(param.accession.as_deref(), Some("MS:9999999"));
        assert!(matches!(param.accession, Some(Cow::Owned(_))));
    }

    #[test]
    fn read_cv_param_gets_an_empty_name_when_the_attribute_is_missing() {
        let param = read_param(r#"<cvParam cvRef="MS" accession="MS:1000511"/>"#);
        assert_eq!(param.name, "");
        assert!(param.value.is_none());
    }
}
