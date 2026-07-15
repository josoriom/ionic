use std::path::{Path, PathBuf};

pub fn ionic() -> assert_cmd::Command {
    assert_cmd::Command::cargo_bin("ionic").unwrap()
}

pub fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../parser/data")
        .canonicalize()
        .unwrap()
}

pub fn ion_fixture(name: &str) -> PathBuf {
    data_dir().join("ion").join(name)
}

pub fn mzml_fixture(name: &str) -> PathBuf {
    data_dir().join("mzml").join(name)
}

pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        for next in chars.by_ref() {
            if next == 'm' {
                break;
            }
        }
    }
    out
}

pub fn read_bytes_to_tempfile_copy(src: &Path, dst: &Path) {
    std::fs::copy(src, dst).unwrap();
}
