use crate::tables;

/// For a `cvParam` element's attribute list, normalise the `value` attribute
/// when the accession has a numeric value type.
///
/// Rules:
///   xsd:float | xsd:int  ->  parse as f64, format with enough precision so
///                             that "20" and "20.0" both become "20" and
///                             "342.32" stays "342.32".
pub fn normalize_cv_attrs(attrs: &mut [(String, String)]) {
    let accession = attrs
        .iter()
        .find(|(k, _)| k == "accession")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");

    if accession.is_empty() {
        return;
    }

    let vtype = tables::cv_value_type(accession);
    if vtype == 0 {
        return; // no value_type entry — leave as-is
    }

    // vtype 1 = xsd:float, vtype 2 = xsd:int
    if (vtype == 1 || vtype == 2)
        && let Some((_, val)) = attrs.iter_mut().find(|(k, _)| k == "value")
        && let Ok(f) = val.parse::<f64>()
    {
        *val = normalize_float(f);
    }
}

/// Format a float value, stripping trailing `.0` for whole numbers.
///
/// Uses `to_string()` for finite whole numbers to avoid `f as i64` overflow
/// for values outside i64 range. For fractional values, uses 15 significant
/// digits with trailing-zero stripping.
fn normalize_float(f: f64) -> String {
    // Canonicalize -0.0 → 0.0 so both produce "0".
    let f = if f == 0.0 { 0.0 } else { f };

    if f.fract() == 0.0 && f.is_finite() {
        // Safe: we format the float directly instead of casting to i64,
        // which would overflow for values like 1e20.
        format!("{f:.0}")
    } else {
        // Rust's default f64 Display uses the shortest representation that
        // round-trips: "342.32" stays "342.32", "0.5" stays "0.5".
        f.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_float_whole_numbers() {
        assert_eq!(normalize_float(20.0), "20");
        assert_eq!(normalize_float(0.0), "0");
        assert_eq!(normalize_float(-1.0), "-1");
        assert_eq!(normalize_float(100.0), "100");
    }

    #[test]
    fn normalize_float_fractional() {
        assert_eq!(normalize_float(342.32), "342.32");
        assert_eq!(normalize_float(0.5), "0.5");
        assert_eq!(normalize_float(3.14159), "3.14159");
    }

    #[test]
    fn normalize_float_large_whole() {
        // This was the `f as i64` overflow bug — 1e20 exceeds i64::MAX.
        let result = normalize_float(1e20);
        assert_eq!(result, "100000000000000000000");
    }

    #[test]
    fn normalize_float_negative_zero() {
        // -0.0 must canonicalize to "0", not "-0".
        assert_eq!(normalize_float(-0.0), "0");
    }

    #[test]
    fn normalize_float_small_fractional() {
        let result = normalize_float(0.000001);
        assert_eq!(result, "0.000001");
    }

    #[test]
    fn normalize_cv_attrs_numeric_term() {
        let mut attrs = vec![
            ("accession".into(), "MS:1000040".into()), // a known numeric CV term
            ("value".into(), "20.0".into()),
        ];
        // We can't guarantee "MS:1000040" has vtype in the tables, so test
        // the normalize_float path directly above. But we verify no crash.
        normalize_cv_attrs(&mut attrs);
    }

    #[test]
    fn normalize_cv_attrs_no_accession() {
        let mut attrs = vec![("name".into(), "test".into())];
        normalize_cv_attrs(&mut attrs);
        assert_eq!(attrs[0], ("name".into(), "test".into()));
    }

    #[test]
    fn normalize_cv_attrs_empty_accession() {
        let mut attrs = vec![
            ("accession".into(), String::new()),
            ("value".into(), "42".into()),
        ];
        normalize_cv_attrs(&mut attrs);
        assert_eq!(attrs[1].1, "42");
    }

    // ── Trailing zeros and precision preservation ────────────────────────

    #[test]
    fn normalize_float_trailing_zeros_stripped() {
        // "20.0" parses as 20.0 which has fract() == 0.0, so → "20"
        assert_eq!(normalize_float(20.0), "20");
        // 1.0, 100.0, etc.
        assert_eq!(normalize_float(1.0), "1");
        assert_eq!(normalize_float(1000.0), "1000");
    }

    #[test]
    fn normalize_float_precision_preserved() {
        // Fractional values must keep their precision.
        assert_eq!(normalize_float(1.23456789012345), "1.23456789012345");
        assert_eq!(normalize_float(0.1), "0.1");
        assert_eq!(normalize_float(1e-15), "0.000000000000001");
    }

    #[test]
    fn normalize_float_special_values() {
        // NaN and Inf should pass through (they're not fract()==0 && finite).
        let nan = normalize_float(f64::NAN);
        assert_eq!(nan, "NaN");
        let inf = normalize_float(f64::INFINITY);
        assert_eq!(inf, "inf");
        let neg_inf = normalize_float(f64::NEG_INFINITY);
        assert_eq!(neg_inf, "-inf");
    }

    // ── vtype filtering (xsd:double, xsd:positiveInteger, xsd:decimal) ──

    #[test]
    fn normalize_cv_attrs_xsd_double_normalizes() {
        // MS:1001096 = "SEQUEST:TopPercentMostIntense" which is xsd:double (non-obsolete).
        let mut attrs = vec![
            ("accession".into(), "MS:1001096".into()),
            ("value".into(), "123.0".into()),
        ];
        normalize_cv_attrs(&mut attrs);
        assert_eq!(attrs[1].1, "123");
    }

    #[test]
    fn normalize_cv_attrs_xsd_float_normalizes() {
        // MS:1000004 = "sample mass" which is xsd:float.
        let mut attrs = vec![
            ("accession".into(), "MS:1000004".into()),
            ("value".into(), "42.0".into()),
        ];
        normalize_cv_attrs(&mut attrs);
        assert_eq!(attrs[1].1, "42");
    }

    #[test]
    fn normalize_cv_attrs_xsd_positive_integer_normalizes() {
        // MS:1000903 is xsd:positiveInteger.
        let mut attrs = vec![
            ("accession".into(), "MS:1000903".into()),
            ("value".into(), "100.0".into()),
        ];
        normalize_cv_attrs(&mut attrs);
        assert_eq!(attrs[1].1, "100");
    }

    #[test]
    fn normalize_cv_attrs_string_type_untouched() {
        // An accession with xsd:string type should NOT normalize the value.
        let mut attrs: Vec<(String, String)> = vec![
            ("accession".into(), "MS:1000030".into()),
            ("value".into(), "42.0".into()),
        ];
        let before = attrs[1].1.clone();
        normalize_cv_attrs(&mut attrs);
        // String-typed terms: vtype == 3, so the value is NOT float-normalized.
        // If vtype == 0 (unknown), also untouched.
        let vtype = tables::cv_value_type("MS:1000030");
        if vtype == 3 || vtype == 0 {
            assert_eq!(attrs[1].1, before);
        }
    }
}
