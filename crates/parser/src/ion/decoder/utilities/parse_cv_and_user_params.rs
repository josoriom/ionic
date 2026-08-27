use std::borrow::Cow;

use crate::{
    cv_table,
    ion::{
        attr_meta::{CV_REF_ATTR, borrow_prefix},
        decoder::decode::{Metadatum, MetadatumValue},
        utilities::common::{is_cv_prefix, unit_cv_ref, value_text},
    },
    mzml::{
        schema::TagId,
        structs::{CvParam, UserParam},
    },
};

#[inline]
pub(crate) fn parse_cv_and_user_params(metadata: &[&Metadatum]) -> (Vec<CvParam>, Vec<UserParam>) {
    let mut cv_params = Vec::with_capacity(metadata.len());
    let mut user_params = Vec::new();

    for entry in metadata {
        if entry.tag_id == TagId::UserParam {
            user_params.push(make_user_param(entry));
            continue;
        }

        let Some(accession) = entry.accession.as_deref() else {
            continue;
        };

        let Some((prefix, _)) = accession.split_once(':') else {
            continue;
        };

        if prefix == CV_REF_ATTR || !is_cv_prefix(prefix) {
            continue;
        }

        cv_params.push(make_cv_param(entry, accession, prefix));
    }

    (cv_params, user_params)
}

#[inline]
fn make_user_param(entry: &Metadatum) -> UserParam {
    let (name, value, type_) = match &entry.value {
        MetadatumValue::Text(s) => {
            let mut parts = s.splitn(3, '\0');
            let name_part = parts.next().unwrap_or("").to_string();
            let value_part = parts
                .next()
                .map(|v| {
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.to_string())
                    }
                })
                .unwrap_or(None);
            let type_part = parts
                .next()
                .map(|t| {
                    if t.is_empty() {
                        None
                    } else {
                        Some(t.to_string())
                    }
                })
                .unwrap_or(None);
            (name_part, value_part, type_part)
        }
        MetadatumValue::Number(n) => (n.to_string(), None, None),
        MetadatumValue::Empty => (String::new(), None, None),
    };

    UserParam {
        name,
        value,
        r#type: type_,
        unit_accession: entry.unit_accession.clone(),
        unit_cv_ref: unit_cv_ref(entry.unit_accession.as_deref()),
        unit_name: None,
    }
}

#[inline]
fn make_cv_param(entry: &Metadatum, accession: &str, prefix: &str) -> CvParam {
    let term = cv_table::find_term(accession);
    let unit_accession = entry.unit_accession.as_deref();
    let unit_term = unit_accession.and_then(cv_table::find_term);

    CvParam {
        cv_ref: Some(borrow_prefix(prefix)),
        accession: Some(match term {
            Some(term) => Cow::Borrowed(term.accession),
            None => Cow::Owned(accession.to_owned()),
        }),
        name: match term {
            Some(term) => Cow::Borrowed(term.name),
            None => Cow::Owned(accession.to_owned()),
        },
        value: value_text(&entry.value).map(Cow::Owned),
        unit_cv_ref: unit_accession
            .and_then(|text| text.split_once(':'))
            .map(|(prefix, _)| borrow_prefix(prefix)),
        unit_name: unit_term.map(|term| Cow::Borrowed(term.name)),
        unit_accession: unit_accession.map(|text| match unit_term {
            Some(term) => Cow::Borrowed(term.accession),
            None => Cow::Owned(text.to_owned()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ion::decoder::decode::Metadatum;

    fn make_entry(accession: &str, value: &str, unit_accession: Option<&str>) -> Metadatum {
        Metadatum {
            item_index: 0,
            id: 0,
            parent_id: 0,
            tag_id: TagId::CvParam,
            accession: Some(accession.to_string()),
            unit_accession: unit_accession.map(|text| text.to_string()),
            value: MetadatumValue::Text(value.to_string()),
        }
    }

    #[test]
    fn make_cv_param_borrows_known_text() {
        let entry = make_entry("MS:1000016", "12.5", Some("UO:0000031"));
        let param = make_cv_param(&entry, "MS:1000016", "MS");

        assert!(matches!(param.cv_ref, Some(Cow::Borrowed(_))));
        assert!(matches!(param.accession, Some(Cow::Borrowed(_))));
        assert!(matches!(param.name, Cow::Borrowed(_)));
        assert!(matches!(param.unit_cv_ref, Some(Cow::Borrowed(_))));
        assert!(matches!(param.unit_accession, Some(Cow::Borrowed(_))));
        assert!(matches!(param.unit_name, Some(Cow::Borrowed(_))));
        assert!(matches!(param.value, Some(Cow::Owned(_))));
    }

    #[test]
    fn make_cv_param_gets_the_expected_text() {
        let entry = make_entry("MS:1000016", "12.5", Some("UO:0000031"));
        let param = make_cv_param(&entry, "MS:1000016", "MS");

        assert_eq!(param.cv_ref.as_deref(), Some("MS"));
        assert_eq!(param.accession.as_deref(), Some("MS:1000016"));
        assert_eq!(param.name, "scan start time");
        assert_eq!(param.value.as_deref(), Some("12.5"));
        assert_eq!(param.unit_cv_ref.as_deref(), Some("UO"));
        assert_eq!(param.unit_accession.as_deref(), Some("UO:0000031"));
        assert_eq!(param.unit_name.as_deref(), Some("minute"));
    }

    #[test]
    fn make_cv_param_owns_an_accession_the_table_does_not_have() {
        let entry = make_entry("MS:9999999", "1", None);
        let param = make_cv_param(&entry, "MS:9999999", "MS");

        assert_eq!(param.accession.as_deref(), Some("MS:9999999"));
        assert_eq!(param.name, "MS:9999999");
        assert!(matches!(param.accession, Some(Cow::Owned(_))));
        assert!(matches!(param.name, Cow::Owned(_)));
        assert!(param.unit_cv_ref.is_none());
        assert!(param.unit_name.is_none());
    }
}
