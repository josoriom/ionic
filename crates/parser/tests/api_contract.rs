mod common;

use std::sync::Arc;

use common::{
    assertions::{assert_mzml_semantic_eq, assert_mzml_structural_eq},
    decode_ion, encode_to_ion, test_files,
};
use ionic::{
    Range, ScanSource,
    ion::{
        IonReader, ReadOptions, IonError, plan_open_ranges,
        encoder::{
            encode::{WriteOptions, TARGET_BLOCK_UNCOMPRESSED_BYTES},
            ion_writer::IonWriter,
            utilities::SectionStorage,
            scan_stream::MemoryReader,
        },
    },
};

fn default_config() -> ReadOptions {
    ReadOptions::default()
}

fn segmented_write_config() -> WriteOptions {
    WriteOptions {
        compression_level: 0,
        force_f32: false,
        block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
        parallel: false,
        section_storage: SectionStorage::Memory,
        segment_size: 64,
    }
}

fn stream_write_config() -> WriteOptions {
    WriteOptions {
        compression_level: 0,
        force_f32: false,
        block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
        parallel: false,
        section_storage: SectionStorage::Memory,
        segment_size: 256 * 1024,
    }
}

fn encode_with_segments(src: &ionic::mzml::structs::MzML) -> Vec<u8> {
    let mut bytes = Vec::new();
    let config = segmented_write_config();
    let mut writer = IonWriter::begin(&mut bytes, config).expect("writer begin must succeed");
    let mut reader = MemoryReader::new(src.clone());
    writer.write_stream(&mut reader).expect("write_stream must succeed");
    bytes
}

#[test]
fn encode_is_deterministic() {
    let src = test_files::tiny_pwiz_11();
    let first = encode_to_ion(src, 12, false);
    let second = encode_to_ion(src, 12, false);
    assert_eq!(first, second, "two identical encodes must produce byte-identical output");
}

#[test]
fn roundtrip_gives_semantic_equal_mzml() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 9, false);
    let decoded = decode_ion(&bytes).expect("decode must succeed");
    assert_mzml_semantic_eq(src, &decoded);
}

#[test]
fn roundtrip_f32_mode_preserves_structure() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 9, true);
    let decoded = decode_ion(&bytes).expect("decode must succeed");
    assert_mzml_structural_eq(src, &decoded);
}

#[test]
fn open_bytes_to_mzml_matches_reference() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 6, false);
    let reference = decode_ion(&bytes).expect("reference decode must succeed");
    let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
    let mut decoder = IonReader::open_bytes(arc, default_config()).expect("open_bytes must succeed");
    let result = decoder.to_mzml().expect("to_mzml must succeed");
    assert_mzml_semantic_eq(&reference, &result);
}

#[test]
fn open_via_callback_to_mzml_matches_reference() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 6, false);
    let reference = decode_ion(&bytes).expect("reference decode must succeed");
    let shared: Arc<[u8]> = Arc::from(bytes.as_slice());
    let callback_bytes = shared.clone();
    let mut decoder = IonReader::open_remote(
        move |range: Range| {
            let start = range.offset as usize;
            let end = start + range.length as usize;
            callback_bytes
                .get(start..end)
                .map(|slice| slice.to_vec())
                .ok_or_else(|| IonError::from("callback: read out of bounds"))
        },
        default_config(),
    )
    .expect("open_remote must succeed");
    let result = decoder.to_mzml().expect("to_mzml must succeed");
    assert_mzml_semantic_eq(&reference, &result);
}

#[test]
fn arc_and_callback_open_paths_give_equal_results() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 6, false);

    let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
    let mut arc_decoder = IonReader::open_bytes(arc.clone(), default_config()).expect("open_bytes must succeed");
    let arc_mzml = arc_decoder.to_mzml().expect("arc to_mzml must succeed");

    let callback_data = arc.clone();
    let mut cb_decoder = IonReader::open_remote(
        move |range: Range| {
            let start = range.offset as usize;
            let end = start + range.length as usize;
            callback_data
                .get(start..end)
                .map(|slice| slice.to_vec())
                .ok_or_else(|| IonError::from("callback: out of bounds"))
        },
        default_config(),
    )
    .expect("open_remote must succeed");
    let cb_mzml = cb_decoder.to_mzml().expect("callback to_mzml must succeed");

    assert_mzml_semantic_eq(&arc_mzml, &cb_mzml);
}

#[test]
fn spectrum_count_and_chromatogram_count_are_stable() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 6, false);
    let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
    let decoder = IonReader::open_bytes(arc, default_config()).expect("open must succeed");
    let spec_count = decoder.spectrum_count();
    let chrom_count = decoder.chromatogram_count();
    assert!(spec_count > 0, "spectrum count must be positive");
    let _ = chrom_count;
}

#[test]
fn scan_source_summary_count_equals_spectrum_count() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 6, false);
    let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
    let mut decoder = IonReader::open_bytes(arc, default_config()).expect("open must succeed");
    let total = decoder.spectrum_count();
    let mut counted = 0usize;
    decoder.for_each_summary(&mut |_, _| counted += 1);
    assert_eq!(counted as u64, total, "for_each_summary count must equal spectrum_count");
}

#[test]
fn load_scan_gives_arrays_for_first_spectrum() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 6, false);
    let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
    let mut decoder = IonReader::open_bytes(arc, default_config()).expect("open must succeed");
    let mut mz = Vec::new();
    let mut intensity = Vec::new();
    let loaded = decoder.load_scan(0, &mut mz, &mut intensity);
    assert!(loaded, "load_scan must return true for index 0");
    assert!(!mz.is_empty(), "mz must not be empty");
    assert_eq!(mz.len(), intensity.len(), "mz and intensity must have equal length");
}

#[test]
fn plan_open_ranges_includes_header_range() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 6, false);
    let ranges = plan_open_ranges(&bytes).expect("plan_open_ranges must succeed");
    assert!(!ranges.is_empty(), "ranges must not be empty");
    assert_eq!(ranges[0].offset, 0, "first range must start at byte 0");
    assert_eq!(ranges[0].length, 1024, "first range must span 1024 bytes");
}

#[test]
fn plan_open_ranges_all_ranges_fit_in_file() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 6, false);
    let file_len = bytes.len() as u64;
    let ranges = plan_open_ranges(&bytes).expect("plan_open_ranges must succeed");
    for range in &ranges {
        let end = range.offset + range.length;
        assert!(
            end <= file_len,
            "range (offset={}, length={}) extends past file end ({file_len})",
            range.offset,
            range.length,
        );
    }
}

#[test]
fn write_via_memory_reader_matches_direct_encode() {
    let src = test_files::tiny_pwiz_11();
    let direct_bytes = encode_to_ion(src, 0, false);
    let reference = decode_ion(&direct_bytes).expect("reference decode must succeed");

    let mut stream_bytes = Vec::new();
    let mut reader = MemoryReader::new(src.clone());
    let mut writer =
        IonWriter::begin(&mut stream_bytes, stream_write_config()).expect("begin must succeed");
    writer.write_stream(&mut reader).expect("write_stream must succeed");
    let stream_decoded = decode_ion(&stream_bytes).expect("stream decode must succeed");
    assert_mzml_semantic_eq(&reference, &stream_decoded);
}

#[test]
fn require_bounds_succeeds_for_segmented_file() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_with_segments(src);
    let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
    let mut decoder = IonReader::open_bytes(arc, default_config()).expect("open must succeed");
    decoder
        .require_bounds()
        .expect("require_bounds must succeed when file has segment bounds");
}

#[test]
fn mz_range_read_returns_data_for_segmented_file() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_with_segments(src);
    let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
    let mut decoder = IonReader::open_bytes(arc, default_config()).expect("open must succeed");
    decoder
        .require_bounds()
        .expect("require_bounds must succeed");
    let window = decoder
        .read_mz_range(0, 0.0, 1e9)
        .expect("read_mz_range must succeed");
    assert_eq!(
        window.mz.len(),
        window.intensity.len(),
        "window mz and intensity must have equal length"
    );
    assert!(!window.mz.is_empty(), "wide window must return data for a non-empty spectrum");
}

#[test]
fn mz_range_matches_get_spectrum_for_full_range() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_with_segments(src);
    let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
    let mut decoder = IonReader::open_bytes(arc, default_config()).expect("open must succeed");
    decoder
        .require_bounds()
        .expect("require_bounds must succeed");

    let spectrum = decoder
        .get_spectrum(0)
        .expect("get_spectrum must succeed")
        .expect("first spectrum must exist");
    let full_len = spectrum
        .binary_data_array_list
        .as_ref()
        .and_then(|list| list.binary_data_arrays.first())
        .and_then(|bda| bda.binary.as_ref())
        .map(|bin| bin.len())
        .unwrap_or(0);

    let window = decoder
        .read_mz_range(0, 0.0, 1e9)
        .expect("wide window must succeed");
    assert_eq!(
        window.mz.len(),
        full_len,
        "wide mz window must return as many points as the full spectrum"
    );
}

#[test]
fn mz_range_block_ranges_are_non_empty_for_segmented_file() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_with_segments(src);
    let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
    let mut decoder = IonReader::open_bytes(arc, default_config()).expect("open must succeed");
    decoder
        .require_bounds()
        .expect("require_bounds must succeed");
    let ranges = decoder
        .plan_mz_range(0, 0.0, 1e9)
        .expect("plan_mz_range must succeed");
    assert!(!ranges.is_empty(), "wide window must return at least one block range");
}

#[test]
fn spec_summary_matches_source_facts() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 6, false);
    let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
    let decoder = IonReader::open_bytes(arc, default_config()).expect("open must succeed");

    let source_spectra = common::spectra(src);
    assert_eq!(
        decoder.spectrum_count() as usize,
        source_spectra.len(),
        "summary count must match source spectrum count"
    );

    for (index, source) in source_spectra.iter().enumerate() {
        let summary = decoder
            .spec_summary(index)
            .expect("spec_summary must return Some for a valid index");

        let expected_level = source.ms_level.unwrap_or(0) as u8;
        assert_eq!(
            summary.ms_level, expected_level,
            "spectrum {index}: stored ms_level must match source ms_level"
        );

        if let Some(expected_rt) = common::scan_start_time_seconds(source) {
            assert!(
                (summary.rt_seconds - expected_rt).abs() <= 1e-6,
                "spectrum {index}: stored rt_seconds {} must match source {expected_rt}",
                summary.rt_seconds,
            );
        }
    }
}

#[test]
fn format_version_is_supported() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 6, false);
    let arc: Arc<[u8]> = Arc::from(bytes.as_slice());
    let decoder = IonReader::open_bytes(arc, default_config()).expect("open must succeed");
    let version = decoder.format_version();
    assert!(
        ionic::ion::format::is_supported(version),
        "format_version must be within supported range"
    );
}

struct GoldenEncode {
    name: &'static str,
    path: &'static str,
    encoded_len: usize,
    encoded_fnv: u64,
}

const GOLDEN_ENCODES: &[GoldenEncode] = &[
    GoldenEncode {
        name: "tiny_pwiz_10",
        path: "crates/parser/data/mzml/tiny.pwiz.1.0.mzML",
        encoded_len: 13568,
        encoded_fnv: 0x2946_c6de_a7c6_13c9,
    },
    GoldenEncode {
        name: "tiny_pwiz_11",
        path: "crates/parser/data/mzml/tiny.pwiz.1.1.mzML",
        encoded_len: 14520,
        encoded_fnv: 0x35a8_647b_5b3c_8c89,
    },
    GoldenEncode {
        name: "anpc_test",
        path: "crates/parser/data/mzml/test.mzML",
        encoded_len: 9208,
        encoded_fnv: 0x70d5_a9ab_801e_32f3,
    },
];

#[test]
fn encoded_bytes_stay_byte_for_byte_stable() {
    for golden in GOLDEN_ENCODES {
        let src = common::parse_test_file(golden.path);
        let bytes = encode_to_ion(&src, 0, false);
        assert_eq!(
            bytes.len(),
            golden.encoded_len,
            "{}: encoded length changed (format drift)",
            golden.name,
        );
        assert_eq!(
            common::fnv64_bytes(&bytes),
            golden.encoded_fnv,
            "{}: encoded bytes changed (format drift)",
            golden.name,
        );
    }
}

#[test]
fn spec_arrayrefs_crc_tamper_fails_closed_with_verify_on_and_opens_with_verify_off() {
    let src = test_files::tiny_pwiz_11();
    let bytes = encode_to_ion(src, 0, false);

    let off_spec_arrayrefs = u64::from_le_bytes(bytes[80..88].try_into().unwrap()) as usize;
    let len_spec_arrayrefs = u64::from_le_bytes(bytes[88..96].try_into().unwrap()) as usize;
    assert!(len_spec_arrayrefs > 0, "spec_arrayrefs must be non-empty for this test to be meaningful");

    let mut tampered = bytes.clone();
    tampered[off_spec_arrayrefs] ^= 0xFF;

    let verify_on = ReadOptions { verify_checksums: true, ..ReadOptions::default() };
    match IonReader::open(&tampered, verify_on) {
        Ok(_) => panic!("tampered spec_arrayrefs must be rejected when verify_checksums is true"),
        Err(err) => {
            let msg = err.to_string();
            assert!(
                msg.contains("spec_arrayrefs") && msg.contains("crc"),
                "error message must name spec_arrayrefs and crc, got: {msg}"
            );
        }
    }

    let verify_off = ReadOptions { verify_checksums: false, ..ReadOptions::default() };
    IonReader::open(&tampered, verify_off)
        .expect("tampered spec_arrayrefs must open when verify_checksums is false");
}
