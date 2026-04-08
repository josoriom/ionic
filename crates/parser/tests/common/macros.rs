#![allow(unused_macros)]

/// Parse a fixture once.
///
/// Usage:
/// ```ignore
/// test_file!(tiny_pwiz_11, "pwiz/example_data/tiny.pwiz.1.1.mzML");
/// ```
macro_rules! test_file {
    ($name:ident, $rel_path:expr) => {
        pub fn $name() -> &'static ionic::mzml::structs::MzML {
            static CACHE: std::sync::OnceLock<ionic::mzml::structs::MzML> =
                std::sync::OnceLock::new();
            CACHE.get_or_init(|| crate::common::parse_test_file($rel_path))
        }
    };
}

/// XML roundtrip: parse → serialize → reparse → assert eq.
///
/// Usage:
/// ```ignore
/// roundtrip_xml!(tiny_10_semantic, tiny_pwiz_10);
/// roundtrip_xml!(small_10_structural, small_pwiz_10, structural);
/// ```
macro_rules! roundtrip_xml {
    ($name:ident, $test_file_fn:path) => {
        #[test]
        fn $name() {
            let original = $test_file_fn();
            let xml = ionic::mzml::bin_to_mzml::bin_to_mzml(original)
                .expect("bin_to_mzml should succeed");
            let reparsed = ionic::mzml::parse_mzml::parse_mzml(xml.as_bytes())
                .expect("reparse should succeed");
            crate::common::assertions::assert_mzml_semantic_eq(original, &reparsed);
        }
    };
    ($name:ident, $test_file_fn:path, structural) => {
        #[test]
        fn $name() {
            let original = $test_file_fn();
            let xml = ionic::mzml::bin_to_mzml::bin_to_mzml(original)
                .expect("bin_to_mzml should succeed");
            let reparsed = ionic::mzml::parse_mzml::parse_mzml(xml.as_bytes())
                .expect("reparse should succeed");
            crate::common::assertions::assert_mzml_structural_eq(original, &reparsed);
        }
    };
}

/// Ion roundtrip: parse → encode → decode → assert semantic eq.
///
/// Usage:
/// ```ignore
/// roundtrip_ion!(tiny_11_level12, tiny_pwiz_11, level = 12);
/// roundtrip_ion!(tiny_11_f32, tiny_pwiz_11, level = 9, f32 = true, structural);
/// ```
macro_rules! roundtrip_ion {
    ($name:ident, $test_file_fn:path, level = $level:expr) => {
        #[test]
        fn $name() {
            let original = $test_file_fn();
            let bytes = crate::common::encode_to_ion(original, $level, false);
            let decoded = crate::common::decode_ion(&bytes).expect("decode should succeed");
            crate::common::assertions::assert_mzml_semantic_eq(original, &decoded);
        }
    };
    ($name:ident, $test_file_fn:path, level = $level:expr, f32 = $f32:expr) => {
        #[test]
        fn $name() {
            let original = $test_file_fn();
            let bytes = crate::common::encode_to_ion(original, $level, $f32);
            let decoded = crate::common::decode_ion(&bytes).expect("decode should succeed");
            crate::common::assertions::assert_mzml_semantic_eq(original, &decoded);
        }
    };
    ($name:ident, $test_file_fn:path, level = $level:expr, f32 = $f32:expr, structural) => {
        #[test]
        fn $name() {
            let original = $test_file_fn();
            let bytes = crate::common::encode_to_ion(original, $level, $f32);
            let decoded = crate::common::decode_ion(&bytes).expect("decode should succeed");
            crate::common::assertions::assert_mzml_structural_eq(original, &decoded);
        }
    };
}

/// Breaker test: clone test_file → mutate → Ion roundtrip → assert attributes survived.
///
/// Usage:
/// ```ignore
/// breaker_test!(scan_attrs, tiny_pwiz_11, |m| { /* mutate */ }, |m| { /* check */ });
/// ```
macro_rules! breaker_test {
    ($name:ident, $fixture_fn:path, $mutator:expr, $checker:expr) => {
        #[test]
        fn $name() {
            let mut mzml = $fixture_fn().clone();
            $mutator(&mut mzml);
            let bytes = crate::common::encode_to_ion(&mzml, 12, false);
            let decoded = crate::common::decode_ion(&bytes).expect("decode should succeed");
            $checker(&decoded);
        }
    };
}
