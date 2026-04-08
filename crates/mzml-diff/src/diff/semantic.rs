use super::structural::Delta;
use crate::tables;
use crate::xml::parse::{AttrKey, NodeKey};

/// Semantic classification for a diff entry. Ordered by severity (highest
/// first) so that a `BTreeMap<SemKind, _>` iteration follows severity order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemKind {
    BinaryData,
    RequiredStructure,
    CvNumeric,
    CvIdentity,
    CvString,
    CvMeta,
    OptionalStructure,
    UserParam,
    Refs,
    Other,
}

impl SemKind {
    pub const ALL: &[Self] = &[
        Self::BinaryData,
        Self::RequiredStructure,
        Self::CvNumeric,
        Self::CvIdentity,
        Self::CvString,
        Self::CvMeta,
        Self::OptionalStructure,
        Self::UserParam,
        Self::Refs,
        Self::Other,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::BinaryData => "binary_data",
            Self::RequiredStructure => "required_structure",
            Self::CvNumeric => "cv_numeric",
            Self::CvIdentity => "cv_identity",
            Self::CvString => "cv_string",
            Self::CvMeta => "cv_meta",
            Self::OptionalStructure => "optional_structure",
            Self::UserParam => "user_param",
            Self::Refs => "refs",
            Self::Other => "other",
        }
    }

    pub const fn severity(self) -> &'static str {
        match self {
            Self::BinaryData | Self::RequiredStructure => "CRITICAL",
            Self::CvNumeric | Self::CvIdentity => "HIGH    ",
            Self::CvString | Self::CvMeta => "MEDIUM  ",
            Self::OptionalStructure | Self::UserParam => "LOW     ",
            Self::Refs | Self::Other => "INFO    ",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::BinaryData => "signal array content changed",
            Self::RequiredStructure => "XSD-required element missing/changed",
            Self::CvNumeric => "numeric cvParam value differs",
            Self::CvIdentity => "cvParam accession changed",
            Self::CvString => "string cvParam value differs",
            Self::CvMeta => "cv name/unit metadata differs",
            Self::OptionalStructure => "optional element absent in one side",
            Self::UserParam => "userParam differs",
            Self::Refs => "cross-reference attribute differs",
            Self::Other => "everything else",
        }
    }
}

/// Extract the last path component (element name).
fn last_elem(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn classify_node(d: &Delta<NodeKey>) -> SemKind {
    let elem = last_elem(&d.key.path);

    if matches!(elem, "binary" | "binaryDataArray" | "binaryDataArrayList") {
        return SemKind::BinaryData;
    }

    if tables::REQUIRED_ELEMENTS.binary_search(&elem).is_ok() {
        return SemKind::RequiredStructure;
    }

    if tables::OPTIONAL_ELEMENTS.binary_search(&elem).is_ok() {
        return SemKind::OptionalStructure;
    }

    SemKind::Other
}

pub fn classify_attr(d: &Delta<AttrKey>) -> SemKind {
    let elem = last_elem(&d.key.path);
    let attr: &str = &d.key.name;

    if tables::IDREF_ATTRS.binary_search(&attr).is_ok() {
        return SemKind::Refs;
    }

    if elem == "userParam" {
        return SemKind::UserParam;
    }

    if elem == "cvParam" {
        return match attr {
            "accession" => SemKind::CvIdentity,
            "value" => {
                // Heuristic: classify as numeric if the value looks like a
                // finite number. We can't look up the CV schema type here
                // because attr deltas don't carry sibling accession context.
                // This is cosmetic (affects severity label, not diff
                // correctness). Filter out Inf/NaN which parse as f64 but
                // aren't meaningful numeric CV values.
                if let Ok(f) = d.key.value.parse::<f64>() {
                    if f.is_finite() {
                        SemKind::CvNumeric
                    } else {
                        SemKind::CvString
                    }
                } else {
                    SemKind::CvString
                }
            }
            _ => SemKind::CvMeta,
        };
    }

    SemKind::Other
}

pub fn classify_text(path: &str) -> SemKind {
    if last_elem(path) == "binary" {
        SemKind::BinaryData
    } else {
        SemKind::Other
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn node_delta(path: &str) -> Delta<NodeKey> {
        Delta {
            key: NodeKey {
                path: Arc::from(path),
                hash: [0; 32],
            },
            left: 1,
            right: 0,
        }
    }

    fn attr_delta(path: &str, name: &str, value: &str) -> Delta<AttrKey> {
        Delta {
            key: AttrKey {
                path: Arc::from(path),
                name: Arc::from(name),
                value: value.into(),
            },
            left: 1,
            right: 0,
        }
    }

    #[test]
    fn classify_binary_node() {
        assert_eq!(
            classify_node(&node_delta("/mzML/run/binary")),
            SemKind::BinaryData
        );
        assert_eq!(
            classify_node(&node_delta("/mzML/run/binaryDataArray")),
            SemKind::BinaryData
        );
    }

    #[test]
    fn classify_cv_accession() {
        assert_eq!(
            classify_attr(&attr_delta("/mzML/cvParam", "accession", "MS:1000514")),
            SemKind::CvIdentity
        );
    }

    #[test]
    fn classify_cv_numeric_value() {
        assert_eq!(
            classify_attr(&attr_delta("/mzML/cvParam", "value", "42.5")),
            SemKind::CvNumeric
        );
    }

    #[test]
    fn classify_cv_string_value() {
        assert_eq!(
            classify_attr(&attr_delta("/mzML/cvParam", "value", "centroid spectrum")),
            SemKind::CvString
        );
    }

    #[test]
    fn classify_user_param() {
        assert_eq!(
            classify_attr(&attr_delta("/mzML/userParam", "name", "x")),
            SemKind::UserParam
        );
    }

    #[test]
    fn classify_text_binary() {
        assert_eq!(classify_text("/mzML/run/binary"), SemKind::BinaryData);
    }

    #[test]
    fn classify_text_other() {
        assert_eq!(classify_text("/mzML/run/name"), SemKind::Other);
    }

    #[test]
    fn classify_cv_nan_inf_as_string() {
        // NaN and Inf parse as f64 but are not finite — should be CvString.
        assert_eq!(
            classify_attr(&attr_delta("/mzML/cvParam", "value", "NaN")),
            SemKind::CvString
        );
        assert_eq!(
            classify_attr(&attr_delta("/mzML/cvParam", "value", "inf")),
            SemKind::CvString
        );
        assert_eq!(
            classify_attr(&attr_delta("/mzML/cvParam", "value", "-inf")),
            SemKind::CvString
        );
    }
}
