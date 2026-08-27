use crate::mzml::structs::CvParam;
use std::borrow::Cow;

pub(crate) const CV_REF_ATTR: &str = "ATTR";

pub(crate) const CV_CODE_MS: u8 = 0;
pub(crate) const CV_CODE_UO: u8 = 1;
pub(crate) const CV_CODE_NCIT: u8 = 2;
pub(crate) const CV_CODE_PEFF: u8 = 3;
pub(crate) const CV_CODE_ATTR: u8 = 4;
pub(crate) const CV_CODE_IMS: u8 = 5;
pub(crate) const CV_CODE_UNKNOWN: u8 = 255;

#[inline]
pub(crate) fn cv_code_from_prefix(prefix: Option<&str>) -> u8 {
    match prefix {
        Some("MS") => CV_CODE_MS,
        Some("UO") => CV_CODE_UO,
        Some("NCIT") => CV_CODE_NCIT,
        Some("PEFF") => CV_CODE_PEFF,
        Some(CV_REF_ATTR) => CV_CODE_ATTR,
        Some("IMS") => CV_CODE_IMS,
        _ => CV_CODE_UNKNOWN,
    }
}

#[inline]
pub(crate) fn cv_prefix_from_code(code: u8) -> Option<&'static str> {
    match code {
        CV_CODE_MS => Some("MS"),
        CV_CODE_UO => Some("UO"),
        CV_CODE_NCIT => Some("NCIT"),
        CV_CODE_PEFF => Some("PEFF"),
        CV_CODE_ATTR => Some(CV_REF_ATTR),
        CV_CODE_IMS => Some("IMS"),
        _ => None,
    }
}

#[inline]
pub(crate) fn borrow_prefix(prefix: &str) -> Cow<'static, str> {
    match cv_prefix_from_code(cv_code_from_prefix(Some(prefix))) {
        Some(known) => Cow::Borrowed(known),
        None => Cow::Owned(prefix.to_owned()),
    }
}

#[inline]
pub(crate) fn borrow_or_own(text: Cow<'_, str>, known: Option<&'static str>) -> Cow<'static, str> {
    match known {
        Some(known) if known == text => Cow::Borrowed(known),
        _ => Cow::Owned(text.into_owned()),
    }
}

#[inline]
pub(crate) fn format_accession(cv_ref_code: u8, tail_raw: u32) -> Option<String> {
    let pref = cv_prefix_from_code(cv_ref_code)?;
    match pref {
        "MS" => Some(format!(
            "MS:{:07}",
            normalize_ms_accession_tail(cv_ref_code, tail_raw)
        )),
        "UO" => Some(format!("UO:{tail_raw:07}")),
        "NCIT" => Some(format!("NCIT:C{tail_raw}")),
        x if x == CV_REF_ATTR => Some(format!("{CV_REF_ATTR}:{tail_raw}")),
        "IMS" => Some(format!("IMS:{tail_raw:07}")),
        _ => Some(format!("{pref}:{tail_raw}")),
    }
}

const MS_ACCESSION_BASE: u32 = 1_000_000;

#[inline]
pub(crate) fn normalize_ms_accession_tail(cv_ref_code: u8, tail: u32) -> u32 {
    if cv_ref_code == CV_CODE_MS && tail != 0 && tail < MS_ACCESSION_BASE {
        MS_ACCESSION_BASE + tail
    } else {
        tail
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AccessionTail(u32);

impl std::fmt::Display for AccessionTail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AccessionTail {
    #[inline]
    pub(crate) fn raw(self) -> u32 {
        self.0
    }

    #[inline]
    pub(crate) fn from_raw(tail: u32) -> Self {
        Self(tail)
    }
}

#[inline]
pub(crate) fn parse_accession_tail(accession: Option<&str>) -> AccessionTail {
    let s = accession.unwrap_or("");
    let tail = s.rsplit_once(':').map(|(_, t)| t).unwrap_or(s);
    let mut v: u32 = 0;
    let mut saw = false;
    for b in tail.bytes() {
        if b.is_ascii_digit() {
            saw = true;
            v = match v
                .checked_mul(10)
                .and_then(|x| x.checked_add((b - b'0') as u32))
            {
                Some(n) => n,
                None => return AccessionTail::from_raw(0),
            };
        }
    }
    AccessionTail::from_raw(if saw { v } else { 0 })
}

pub(crate) const ACC_ATTR_ID: AccessionTail = AccessionTail(9_910_001);
pub(crate) const ACC_ATTR_REF: AccessionTail = AccessionTail(9_910_002);
pub(crate) const ACC_ATTR_NAME: AccessionTail = AccessionTail(9_910_003);
pub(crate) const ACC_ATTR_LOCATION: AccessionTail = AccessionTail(9_910_004);
pub(crate) const ACC_ATTR_CV_FULL_NAME: AccessionTail = AccessionTail(9_900_002);
pub(crate) const ACC_ATTR_CV_VERSION: AccessionTail = AccessionTail(9_900_003);
pub(crate) const ACC_ATTR_CV_URI: AccessionTail = AccessionTail(9_900_004);
pub(crate) const ACC_ATTR_LABEL: AccessionTail = AccessionTail(9_910_020);

pub(crate) const ACC_ATTR_START_TIME_STAMP: AccessionTail = AccessionTail(9_910_005);
pub(crate) const ACC_ATTR_DEFAULT_INSTRUMENT_CONFIGURATION_REF: AccessionTail =
    AccessionTail(9_910_006);
pub(crate) const ACC_ATTR_DEFAULT_SOURCE_FILE_REF: AccessionTail = AccessionTail(9_910_007);
pub(crate) const ACC_ATTR_SAMPLE_REF: AccessionTail = AccessionTail(9_910_008);

pub(crate) const ACC_ATTR_DEFAULT_DATA_PROCESSING_REF: AccessionTail = AccessionTail(9_910_009);
pub(crate) const ACC_ATTR_DATA_PROCESSING_REF: AccessionTail = AccessionTail(9_910_010);
pub(crate) const ACC_ATTR_SOURCE_FILE_REF: AccessionTail = AccessionTail(9_910_011);

pub(crate) const ACC_ATTR_NATIVE_ID: AccessionTail = AccessionTail(9_910_012);
pub(crate) const ACC_ATTR_SPOT_ID: AccessionTail = AccessionTail(9_910_013);
pub(crate) const ACC_ATTR_EXTERNAL_SPECTRUM_ID: AccessionTail = AccessionTail(9_910_014);
pub(crate) const ACC_ATTR_SPECTRUM_REF: AccessionTail = AccessionTail(9_910_015);

pub(crate) const ACC_ATTR_SCAN_SETTINGS_REF: AccessionTail = AccessionTail(9_910_016);
pub(crate) const ACC_ATTR_INSTRUMENT_CONFIGURATION_REF: AccessionTail = AccessionTail(9_910_017);

pub(crate) const ACC_ATTR_SOFTWARE_REF: AccessionTail = AccessionTail(9_910_018);
pub(crate) const ACC_ATTR_VERSION: AccessionTail = AccessionTail(9_910_019);

pub(crate) const ACC_ATTR_COUNT: AccessionTail = AccessionTail(9_910_100);
pub(crate) const ACC_ATTR_ORDER: AccessionTail = AccessionTail(9_910_101);
pub(crate) const ACC_ATTR_INDEX: AccessionTail = AccessionTail(9_910_102);
pub(crate) const ACC_ATTR_SCAN_NUMBER: AccessionTail = AccessionTail(9_910_103);
pub(crate) const ACC_ATTR_DEFAULT_ARRAY_LENGTH: AccessionTail = AccessionTail(9_910_104);
pub(crate) const ACC_ATTR_ARRAY_LENGTH: AccessionTail = AccessionTail(9_910_105);
pub(crate) const ACC_ATTR_ENCODED_LENGTH: AccessionTail = AccessionTail(9_910_106);
pub(crate) const ACC_ATTR_MS_LEVEL: AccessionTail = AccessionTail(9_910_107);

#[inline]
pub(crate) fn attr_cv_param(tail: AccessionTail, value: &str) -> CvParam {
    CvParam {
        cv_ref: Some(Cow::Borrowed(CV_REF_ATTR)),
        accession: Some(Cow::Owned(format!("{}:{:07}", CV_REF_ATTR, tail.raw()))),
        name: Cow::Borrowed(""),
        value: Some(Cow::Owned(value.to_string())),
        unit_cv_ref: None,
        unit_name: None,
        unit_accession: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrow_prefix_borrows_known_prefixes() {
        for prefix in ["MS", "UO", "NCIT", "PEFF", "IMS", CV_REF_ATTR] {
            let text = borrow_prefix(&String::from(prefix));
            assert!(matches!(text, Cow::Borrowed(_)), "{prefix} should borrow");
            assert_eq!(text, prefix);
        }
    }

    #[test]
    fn borrow_prefix_owns_unknown_prefixes() {
        let text = borrow_prefix("VENDOR");
        assert!(matches!(text, Cow::Owned(_)));
        assert_eq!(text, "VENDOR");
    }

    #[test]
    fn borrow_or_own_borrows_when_the_text_matches() {
        let text = borrow_or_own(Cow::Owned(String::from("ms level")), Some("ms level"));
        assert!(matches!(text, Cow::Borrowed(_)));
        assert_eq!(text, "ms level");
    }

    #[test]
    fn borrow_or_own_owns_when_the_text_differs() {
        let text = borrow_or_own(Cow::Borrowed("custom level"), Some("ms level"));
        assert!(matches!(text, Cow::Owned(_)));
        assert_eq!(text, "custom level");
    }

    #[test]
    fn borrow_or_own_owns_when_nothing_is_known() {
        let text = borrow_or_own(Cow::Borrowed("vendor name"), None);
        assert!(matches!(text, Cow::Owned(_)));
        assert_eq!(text, "vendor name");
    }

    #[test]
    fn parse_accession_tail_roundtrip() {
        assert_eq!(parse_accession_tail(Some("MS:1000514")).raw(), 1_000_514);
        assert_eq!(parse_accession_tail(Some("ATTR:9910001")).raw(), 9_910_001);
        assert_eq!(parse_accession_tail(None).raw(), 0);
        assert_eq!(parse_accession_tail(Some("no-colon")).raw(), 0);
    }

    #[test]
    fn attr_cv_param_round_trip() {
        let cv = attr_cv_param(ACC_ATTR_ID, "scan=1");
        assert_eq!(cv.cv_ref.as_deref(), Some(CV_REF_ATTR));
        assert_eq!(cv.value.as_deref(), Some("scan=1"));
        assert!(cv.accession.as_deref().unwrap().contains("9910001"));
    }

    #[test]
    fn cv_ref_code_round_trip() {
        for code in [
            CV_CODE_MS,
            CV_CODE_UO,
            CV_CODE_NCIT,
            CV_CODE_PEFF,
            CV_CODE_ATTR,
        ] {
            let prefix = cv_prefix_from_code(code).unwrap();
            assert_eq!(cv_code_from_prefix(Some(prefix)), code);
        }
    }
}
