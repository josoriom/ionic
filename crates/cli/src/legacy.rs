use std::{
    fs,
    io::{Write, stderr, stdout},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
};

use ionic::{IonReader, ReadOptions, upgrade_old_ion};
use rayon::prelude::*;

use crate::{basename, collect_files_with_exts};

fn update_one(file: &Path, output_root: &Path, dry_run: bool) -> Result<(u64, u64), String> {
    let old_bytes = fs::read(file).map_err(|e| format!("read failed: {e}"))?;

    let new_bytes = upgrade_old_ion(&old_bytes).map_err(|e| e.to_string())?;

    let mut reader =
        IonReader::open(&new_bytes, ReadOptions::default()).map_err(|e| e.to_string())?;
    reader.to_mzml().map_err(|e| e.to_string())?;
    reader.require_bounds().map_err(|e| e.to_string())?;

    let old_len = old_bytes.len() as u64;
    let new_len = new_bytes.len() as u64;

    if !dry_run {
        let name = file.file_name().ok_or("file has no name")?;
        let out_path = output_root.join(name);
        let temp_path = out_path.with_extension("ion.tmp");
        fs::write(&temp_path, &new_bytes).map_err(|e| format!("write temp failed: {e}"))?;
        fs::rename(&temp_path, &out_path).map_err(|e| format!("rename failed: {e}"))?;
    }

    Ok((old_len, new_len))
}

pub(crate) fn run_update(
    input_root: &Path,
    output_root: &Path,
    filter: Option<&dyn Fn(&str) -> bool>,
    dry_run: bool,
    pool: &rayon::ThreadPool,
    across_files: bool,
) -> Result<(), String> {
    const MB: f64 = 1024.0 * 1024.0;

    let files = collect_files_with_exts(input_root, &["ion"], filter)?;
    if files.is_empty() {
        return Err(format!(
            "no matching .ion files found under {}",
            input_root.display()
        ));
    }

    let total = files.len();
    let ok = AtomicU32::new(0);
    let failed = AtomicU32::new(0);
    let done = AtomicUsize::new(0);
    let print_lock = Mutex::new(());

    let update_file = |file: &PathBuf| {
        let name = basename(file);
        match update_one(file, output_root, dry_run) {
            Ok((old_len, new_len)) => {
                ok.fetch_add(1, Ordering::Relaxed);
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                let tag = if dry_run { "[dry-run]" } else { "[ok]" };
                let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                println!(
                    "{tag} [{n}/{total}] {name}  {:.2} MB -> {:.2} MB",
                    old_len as f64 / MB,
                    new_len as f64 / MB
                );
                let _ = stdout().flush();
            }
            Err(e) => {
                failed.fetch_add(1, Ordering::Relaxed);
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                eprintln!("[error] [{n}/{total}] {name}: {e}");
                let _ = stderr().flush();
            }
        }
    };

    pool.install(|| {
        if across_files {
            files.par_iter().for_each(update_file);
        } else {
            files.iter().for_each(update_file);
        }
    });

    let ok = ok.load(Ordering::Relaxed);
    let failed = failed.load(Ordering::Relaxed);
    println!("updated={ok} failed={failed} total={total} dry_run={dry_run}");
    if failed > 0 {
        return Err(format!("{failed} file(s) failed to update"));
    }
    Ok(())
}
