mod common;

use common::assertions::*;
use common::test_files;
use ionic::mzml::{bin_to_mzml::bin_to_mzml, parse_mzml::parse_indexed_mzml};

#[test]
fn test_file_indices_match_model() {
    let rel = "crates/parser/data/mzml/tiny.pwiz.mzML0.99.10.mzML";
    let indexed = common::parse_indexed(rel);
    assert_index_offsets_match_model(&indexed, rel);
}

#[test]
fn test_test_file_preserves_raw_index_entries() {
    let indexed = common::parse_indexed("crates/parser/data/mzml/test.mzML");
    assert_eq!(indexed.index_list.spectrum.len(), 2);
    assert_eq!(indexed.index_list.chromatogram.len(), 2);
    assert_eq!(
        indexed.index_list.spectrum[0].id_ref.as_deref(),
        Some("scan=1")
    );
    assert_eq!(
        indexed.index_list.spectrum[1].id_ref.as_deref(),
        Some("scan=2")
    );
    assert_eq!(
        indexed.index_list.chromatogram[0].id_ref.as_deref(),
        Some("TIC")
    );
    assert_eq!(
        indexed.index_list.chromatogram[1].id_ref.as_deref(),
        Some("BPC")
    );
    assert!(
        indexed
            .index_list
            .spectrum
            .iter()
            .all(|offset| offset.offset > 0)
    );
    assert!(
        indexed
            .index_list
            .chromatogram
            .iter()
            .all(|offset| offset.offset > 0)
    );
    assert!(indexed.index_list_offset.is_some());
}

#[test]
fn serializer_emits_parseable_index_entries() {
    let test_files_list: [(&str, &ionic::mzml::structs::MzML); 3] = [
        ("tiny.pwiz.1.1", test_files::tiny_pwiz_11()),
        ("small.pwiz.1.1", test_files::small_pwiz_11()),
        ("small_zlib.pwiz.1.1", test_files::small_zlib_pwiz_11()),
    ];

    for (label, src) in test_files_list {
        let xml =
            bin_to_mzml(src).unwrap_or_else(|e| panic!("bin_to_mzml failed for {label}: {e}"));
        let indexed = parse_indexed_mzml(&xml)
            .unwrap_or_else(|e| panic!("parse_indexed_mzml failed for generated {label}: {e}"));
        assert_mzml_semantic_eq(src, &indexed.mzml);
        assert_index_offsets_match_model(&indexed, label);
        assert!(
            indexed.index_list_offset.is_some(),
            "generated indexed mzML should include indexListOffset for {label}"
        );
    }
}
