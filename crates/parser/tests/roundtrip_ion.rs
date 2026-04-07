mod common;

use common::assertions::*;
use common::fixtures;
use common::{chromatograms, decode_ion, encode_to_ion, spectra};

// Tests 7-12

#[test]
fn tiny_11_level12_with_header_check() {
    let src = fixtures::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 12, false);
    assert_eq!(
        &bytes[..8],
        b"START\0\0\0",
        "encoded header signature must be START\\0\\0\\0"
    );
    let decoded = decode_ion(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(src, &decoded);
}

#[test]
fn tiny_10_level0_with_header_check() {
    let src = fixtures::tiny_pwiz_10();
    let bytes = encode_to_ion(src, 0, false);
    assert_eq!(&bytes[..8], b"START\0\0\0");
    let decoded = decode_ion(&bytes).expect("decode should succeed");
    assert_mzml_semantic_eq(src, &decoded);
}

#[test]
fn f32_mode_keeps_structure() {
    let src = fixtures::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 9, true);
    let decoded = decode_ion(&bytes).expect("decode should succeed");
    assert_mzml_structural_eq(src, &decoded);
}

#[test]
fn config_matrix_identity_smoke() {
    let fixtures_list = [
        fixtures::tiny_pwiz_10(),
        fixtures::tiny_pwiz_11(),
        fixtures::anpc_test_mzml(),
    ];
    let levels = [0_u8, 3_u8, 6_u8, 9_u8, 12_u8];
    let modes = [false, true];

    for (fi, src) in fixtures_list.iter().enumerate() {
        for level in levels {
            for to_f32 in modes {
                let bytes = encode_to_ion(src, level, to_f32);
                let out = decode_ion(&bytes).expect("decode should succeed");

                let src_spec_ids: Vec<_> = spectra(src).iter().map(|s| s.id.as_str()).collect();
                let out_spec_ids: Vec<_> = spectra(&out).iter().map(|s| s.id.as_str()).collect();
                assert_eq!(
                    src_spec_ids, out_spec_ids,
                    "spectrum ids changed: fixture#{fi} level={level} f32={to_f32}"
                );

                let src_chrom_ids: Vec<_> =
                    chromatograms(src).iter().map(|c| c.id.as_str()).collect();
                let out_chrom_ids: Vec<_> =
                    chromatograms(&out).iter().map(|c| c.id.as_str()).collect();
                assert_eq!(
                    src_chrom_ids, out_chrom_ids,
                    "chrom ids changed: fixture#{fi} level={level} f32={to_f32}"
                );
            }
        }
    }
}

#[test]
fn roundtrip_across_levels() {
    let src = fixtures::tiny_pwiz_11();
    for level in [0_u8, 3_u8, 12_u8] {
        let bytes = encode_to_ion(src, level, false);
        let out = decode_ion(&bytes).expect("decode should succeed");
        assert_mzml_semantic_eq(src, &out);
    }
}
