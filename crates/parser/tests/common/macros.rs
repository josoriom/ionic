#![allow(unused_macros)]

macro_rules! test_file {
    ($name:ident, $rel_path:expr) => {
        pub fn $name() -> &'static ionic::mzml::structs::MzML {
            static CACHE: std::sync::OnceLock<ionic::mzml::structs::MzML> =
                std::sync::OnceLock::new();
            CACHE.get_or_init(|| crate::common::parse_test_file($rel_path))
        }
    };
}

macro_rules! roundtrip_xml {
    ($name:ident, $test_file_fn:path) => {
        #[test]
        fn $name() {
            let original = $test_file_fn();
            let xml = ionic::mzml::bin_to_mzml::bin_to_mzml(original)
                .expect("bin_to_mzml should succeed");
            let reparsed =
                ionic::mzml::parse_mzml::parse_mzml(&xml).expect("reparse should succeed");
            crate::common::assertions::assert_mzml_semantic_eq(original, &reparsed);
        }
    };
    ($name:ident, $test_file_fn:path, structural) => {
        #[test]
        fn $name() {
            let original = $test_file_fn();
            let xml = ionic::mzml::bin_to_mzml::bin_to_mzml(original)
                .expect("bin_to_mzml should succeed");
            let reparsed =
                ionic::mzml::parse_mzml::parse_mzml(&xml).expect("reparse should succeed");
            crate::common::assertions::assert_mzml_structural_eq(original, &reparsed);
        }
    };
}

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
