use std::path::PathBuf;

use ionic::{
    ion::encoder::{
        encode::{TARGET_BLOCK_UNCOMPRESSED_BYTES, WriteOptions},
        ion_writer::write_mzml_to_ion,
        utilities::SectionStorage,
    },
    mzml::parse_mzml::parse_mzml,
};

fn mzml_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join("mzml")
}

fn mzml_files() -> Vec<PathBuf> {
    std::fs::read_dir(mzml_dir())
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            let is_mzml = path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("mzml"));
            if is_mzml { Some(path) } else { None }
        })
        .collect()
}

fn config(mode: SectionStorage, compression_level: u8) -> WriteOptions {
    WriteOptions {
        compression_level,
        force_f32: false,
        block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
        parallel: false,
        section_storage: mode,
        mz_window: 0.0,
    }
}

#[test]
fn disk_chuncks_matches_memory_bytes() {
    let files = mzml_files();
    assert!(!files.is_empty(), "no mzml fixtures found");

    let small_file_limit = 1024 * 1024;
    let mut checked = 0;

    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        if bytes.len() >= small_file_limit {
            continue;
        }
        let mzml = parse_mzml(&bytes).unwrap();
        let name = path.file_name().unwrap().to_string_lossy();

        for level in [0u8, 3] {
            let mut memory = Vec::new();
            write_mzml_to_ion(&mzml, config(SectionStorage::Memory, level), &mut memory).unwrap();
            let mut disk = Vec::new();
            write_mzml_to_ion(&mzml, config(SectionStorage::Disk, level), &mut disk).unwrap();

            assert_eq!(
                memory, disk,
                "{name}: disk-staged bytes differ from memory-staged at compression level {level}"
            );
        }
        checked += 1;
    }

    assert!(checked > 0, "no committed mzml fixtures under size limit");
}
