#![cfg(unix)]

mod common;
use common::*;

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                stack.push(entry.path());
            } else {
                found.push(entry.path());
            }
        }
    }
    found
}

fn count_ion_files(root: &Path) -> usize {
    walk(root)
        .iter()
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("ion"))
        .count()
}

fn has_loop_dir(root: &Path) -> bool {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if entry.file_name() == "loop" {
                    return true;
                }
                stack.push(entry.path());
            }
        }
    }
    false
}

#[test]
fn symlink_cycle_terminates() {
    let input_dir = tempfile::TempDir::new().unwrap();
    read_bytes_to_tempfile_copy(
        &mzml_fixture("tiny.msdata.mzML0.99.9.mzML"),
        &input_dir.path().join("tiny.msdata.mzML0.99.9.mzML"),
    );
    symlink(input_dir.path(), input_dir.path().join("loop")).unwrap();

    let output_dir = tempfile::TempDir::new().unwrap();

    let mut cmd = ionic();
    cmd.arg("convert")
        .arg("-i")
        .arg(input_dir.path())
        .arg("-o")
        .arg(output_dir.path())
        .timeout(std::time::Duration::from_secs(30));
    let output = cmd.output().unwrap();

    assert!(
        output.status.success(),
        "a symlink cycle should not prevent the walk from terminating, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let ion_count = count_ion_files(output_dir.path());
    assert_eq!(
        ion_count, 1,
        "the symlink cycle must convert the real file exactly once, found {ion_count} .ion files"
    );

    assert!(
        !has_loop_dir(output_dir.path()),
        "the cycle symlink must never be descended into the output tree"
    );
}
