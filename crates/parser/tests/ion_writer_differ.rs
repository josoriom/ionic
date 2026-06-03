mod common;

use std::path::PathBuf;

use ionic::{
    ion::{
        DecoderConfig, Ion,
        encoder::encode::{EncodingConfig, TARGET_BLOCK_UNCOMPRESSED_BYTES, WritingMode},
        encoder::ion_writer::write_mzml_to_ion,
        encoder::encode::encode as old_encode,
    },
    mzml::parse_mzml::parse_mzml,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data").join("ion").join(name)
}

fn ion_files() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data").join("ion");
    std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("ion") {
                Some(p)
            } else {
                None
            }
        })
        .collect()
}

fn decode_all(bytes: &[u8]) -> ionic::mzml::structs::MzML {
    let mut ion = Ion::open(bytes, DecoderConfig::default()).unwrap();
    ion.to_mzml().unwrap()
}

fn mzml_fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data").join("mzml").join(name);
    std::fs::read(path).unwrap()
}

fn mzml_files() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data").join("mzml");
    std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            if p.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()) == Some("mzml".to_string()) {
                Some(p)
            } else {
                None
            }
        })
        .collect()
}

fn config_plain() -> EncodingConfig {
    EncodingConfig {
        compression_level: 0,
        force_f32: false,
        writing_mode: WritingMode::Streaming,
        uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
        parallel: false,
    }
}

fn config_compressed() -> EncodingConfig {
    EncodingConfig {
        compression_level: 3,
        force_f32: false,
        writing_mode: WritingMode::Streaming,
        uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
        parallel: false,
    }
}

#[test]
fn new_writer_matches_old_encoder_semantically_plain() {
    let files = mzml_files();
    assert!(!files.is_empty(), "no mzml fixtures found");

    for path in &files {
        let mzml_bytes = std::fs::read(path).unwrap();
        let mzml = parse_mzml(&mzml_bytes).unwrap();
        let config = config_plain();

        let mut old_out: Vec<u8> = Vec::new();
        old_encode(&mzml, config.compression_level, config.force_f32, WritingMode::Streaming, &mut old_out).unwrap();

        let mut new_out: Vec<u8> = Vec::new();
        write_mzml_to_ion(&mzml, config, &mut new_out).unwrap();

        let old_decoded = decode_all(&old_out);
        let new_decoded = decode_all(&new_out);

        let name = path.file_name().unwrap().to_string_lossy();
        assert_eq!(
            old_decoded.run.id, new_decoded.run.id,
            "{name}: run id mismatch"
        );

        let old_specs = old_decoded.run.spectrum_list.as_ref().map_or(0, |l| l.spectra.len());
        let new_specs = new_decoded.run.spectrum_list.as_ref().map_or(0, |l| l.spectra.len());
        assert_eq!(old_specs, new_specs, "{name}: spectrum count mismatch");

        let old_chroms = old_decoded.run.chromatogram_list.as_ref().map_or(0, |l| l.chromatograms.len());
        let new_chroms = new_decoded.run.chromatogram_list.as_ref().map_or(0, |l| l.chromatograms.len());
        assert_eq!(old_chroms, new_chroms, "{name}: chromatogram count mismatch");

        if let (Some(old_sl), Some(new_sl)) = (
            old_decoded.run.spectrum_list.as_ref(),
            new_decoded.run.spectrum_list.as_ref(),
        ) {
            for (i, (old_spec, new_spec)) in old_sl.spectra.iter().zip(new_sl.spectra.iter()).enumerate() {
                assert_eq!(old_spec.id, new_spec.id, "{name}: spectrum {i} id mismatch");
                assert_eq!(
                    old_spec.binary_data_array_list.is_some(),
                    new_spec.binary_data_array_list.is_some(),
                    "{name}: spectrum {i} array list presence mismatch"
                );
                if let (Some(old_bd), Some(new_bd)) = (
                    old_spec.binary_data_array_list.as_ref(),
                    new_spec.binary_data_array_list.as_ref(),
                ) {
                    assert_eq!(
                        old_bd.binary_data_arrays.len(),
                        new_bd.binary_data_arrays.len(),
                        "{name}: spectrum {i} array count mismatch"
                    );
                }
            }
        }
    }
}

#[test]
fn new_writer_matches_old_encoder_semantically_compressed() {
    let files = mzml_files();
    assert!(!files.is_empty(), "no mzml fixtures found");

    for path in &files {
        let mzml_bytes = std::fs::read(path).unwrap();
        let mzml = parse_mzml(&mzml_bytes).unwrap();
        let config = config_compressed();

        let mut old_out: Vec<u8> = Vec::new();
        old_encode(&mzml, config.compression_level, config.force_f32, WritingMode::Streaming, &mut old_out).unwrap();

        let mut new_out: Vec<u8> = Vec::new();
        write_mzml_to_ion(&mzml, config, &mut new_out).unwrap();

        let old_decoded = decode_all(&old_out);
        let new_decoded = decode_all(&new_out);

        let name = path.file_name().unwrap().to_string_lossy();
        let old_specs = old_decoded.run.spectrum_list.as_ref().map_or(0, |l| l.spectra.len());
        let new_specs = new_decoded.run.spectrum_list.as_ref().map_or(0, |l| l.spectra.len());
        assert_eq!(old_specs, new_specs, "{name}: spectrum count mismatch (compressed)");
    }
}
