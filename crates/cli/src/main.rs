use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write, stderr, stdout},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
    time::Instant,
};

use clap::{
    ArgAction, ArgGroup, Args, ColorChoice, CommandFactory, FromArgMatches, Parser, Subcommand,
    ValueEnum,
    builder::styling::{AnsiColor, Color, Style, Styles},
};
use ionic::{
    ion::{
        FileWriter, IonReader, ReadOptions,
        encoder::{encode::WriteOptions, ion_writer::IonWriter, utilities::SectionStorage},
        format::FILE_TRAILER,
    },
    mzml::{MzmlReader, bin_to_mzml::bin_to_mzml, parse_mzml::parse_mzml, structs::*},
};
use mimalloc::MiMalloc;
use rayon::{ThreadPoolBuilder, prelude::*};
use regex::Regex;
use serde::Serialize;

mod legacy;
mod utilities;

use utilities::{TempOutput, check_ion_file, sweep_orphans};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_GREEN: &str = "\x1b[1;32m";
const ANSI_YELLOW: &str = "\x1b[1;33m";
const ANSI_RED: &str = "\x1b[1;31m";
const ANSI_BLUE: &str = "\x1b[1;34m";

const AFTER_HELP: &str = "
\x1b[1;33mQUICK REFERENCE\x1b[0m (full flags are in `ionic convert --help` / `ionic cat --help`)

\x1b[1;32mUSAGE:\x1b[0m
  \x1b[96mionic convert\x1b[0m [--mzml-to-ion | --ion-to-mzml]
               -i, --input-path DIR
               -o, --output-path DIR

  \x1b[96mionic cat\x1b[0m [--check] PATH

\x1b[1;32mOPTIONS:\x1b[0m
  \x1b[96m-h\x1b[0m, \x1b[96m--help\x1b[0m
  \x1b[96m-v\x1b[0m, \x1b[96m--version\x1b[0m

\x1b[1;32mEXAMPLES:\x1b[0m
  \x1b[96mionic convert\x1b[0m -i crates/parser/data/mzml -o crates/parser/data/ion
  \x1b[96mionic convert\x1b[0m --ion-to-mzml -i crates/parser/data/ion -o crates/parser/data/mzml_out
  \x1b[96mionic cat\x1b[0m crates/parser/data/ion/tiny.msdata.mzML0.99.9.ion
";

fn cli_styles() -> Styles {
    Styles::styled().literal(Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))))
}

#[derive(Parser)]
#[command(
    name = "ionic",
    version = VERSION,
    arg_required_else_help = true,
    disable_help_subcommand = true,
    disable_version_flag = true
)]
struct Cli {
    #[arg(short = 'v', long = "version", action = ArgAction::SetTrue, global = true, help = "Print the version")]
    version: bool,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    Convert(ConvertArgs),
    Cat(CatArgs),
}

#[derive(Args)]
#[command(
    group(
        ArgGroup::new("convert_mode")
            .args(["mzml_to_ion", "ion_to_mzml"])
            .multiple(false)
    ),
    group(
        ArgGroup::new("pattern_mode")
            .args(["pattern", "pattern_exact", "regex"])
            .multiple(false)
    )
)]
struct ConvertArgs {
    #[arg(
        short = 'i',
        long = "input-path",
        required = true,
        help = "File or folder to convert"
    )]
    input_path: PathBuf,

    #[arg(
        short = 'o',
        long = "output-path",
        help = "Folder to write results into"
    )]
    output_path: Option<PathBuf>,

    #[arg(
        long = "level",
        default_value_t = 22,
        value_parser = clap::value_parser!(u8).range(0..=22),
        help = "Compression level 0–22 (0 = off)"
    )]
    compression_level: u8,

    #[arg(
        long = "block-size",
        default_value_t = 8.0,
        value_name = "MB",
        help = "Block size in MB before compression"
    )]
    block_size_mb: f64,

    #[arg(long, default_value_t = false, action = ArgAction::SetTrue, help = "Overwrite output files that exist")]
    overwrite: bool,

    #[arg(
        long = "pattern",
        value_name = "TEXT",
        help = "Only files whose name contains TEXT"
    )]
    pattern: Option<String>,

    #[arg(
        long = "pattern-exact",
        value_name = "NAME",
        help = "Only files named exactly NAME"
    )]
    pattern_exact: Option<String>,

    #[arg(
        long = "regex",
        value_name = "REGEX",
        help = "Only files matching REGEX"
    )]
    regex: Option<String>,

    #[arg(
        long = "cores",
        default_value_t = 0u16,
        value_parser = clap::value_parser!(u16).range(0..=1024),
        value_name = "N",
        help = "Worker threads (0 = all cores)"
    )]
    cores: u16,

    #[arg(long = "many-files", default_value_t = false, action = ArgAction::SetTrue, help = "Faster for many small files")]
    many_files: bool,

    #[arg(
        long = "storage",
        value_enum,
        value_name = "MODE",
        default_value_t = SectionStorageArg::Disk,
        hide_default_value = true,
        hide_possible_values = true,
        help = "Stage sections [options: memory, disk (default)]"
    )]
    section_storage: SectionStorageArg,

    #[arg(long = "update", default_value_t = false, action = ArgAction::SetTrue)]
    update: bool,

    #[arg(
        long = "mz-window",
        default_value_t = 250.0,
        value_name = "DA",
        help = "m/z split width in Da (smaller = read less)"
    )]
    mz_window: f64,

    #[arg(long = "dry-run", default_value_t = false, action = ArgAction::SetTrue)]
    dry_run: bool,

    #[arg(long = "force-f32", default_value_t = false, action = ArgAction::SetTrue, help = "Store f64 arrays as 32-bit floats (smaller, lossy)")]
    force_f32: bool,

    #[command(flatten)]
    which: ConvertWhich,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SectionStorageArg {
    Memory,
    Disk,
}

impl SectionStorageArg {
    fn storage(self) -> SectionStorage {
        match self {
            Self::Memory => SectionStorage::Memory,
            Self::Disk => SectionStorage::Disk,
        }
    }
}

#[derive(Args)]
struct ConvertWhich {
    #[arg(long = "mzml-to-ion", help = "Convert mzML to ion (default)")]
    mzml_to_ion: bool,

    #[arg(long = "ion-to-mzml", help = "Convert ion back to mzML")]
    ion_to_mzml: bool,

    #[arg(long = "benchmark-decode")]
    benchmark_decode: bool,
}

#[derive(Clone, Copy)]
enum Encoding {
    Parallel,
    Sequential,
}

#[derive(Args)]
#[command(
    group(
        ArgGroup::new("item")
            .args(["check", "scan", "scan_full", "chrom", "chrom_full"])
            .multiple(false)
    )
)]
struct CatArgs {
    #[arg(value_name = "PATH", help = "File to read (.ion or .mzML)")]
    file_path: PathBuf,

    #[arg(long = "full", short = 'f', action = ArgAction::SetTrue, default_value_t = false, conflicts_with = "item", help = "Show all metadata, not a summary")]
    full: bool,

    #[arg(long = "check", action = ArgAction::SetTrue, default_value_t = false, help = "Validate the file and report integrity")]
    check: bool,

    #[arg(long = "scan", value_name = "N", value_parser = clap::value_parser!(u32).range(1..), help = "Metadata of the Nth spectrum (1-based)")]
    scan: Option<u32>,

    #[arg(long = "scan-full", value_name = "N", value_parser = clap::value_parser!(u32).range(1..), help = "Metadata + arrays of the Nth spectrum (1-based)")]
    scan_full: Option<u32>,

    #[arg(long = "chrom", value_name = "N", value_parser = clap::value_parser!(u32).range(1..), help = "Metadata of the Nth chromatogram (1-based)")]
    chrom: Option<u32>,

    #[arg(long = "chrom-full", value_name = "N", value_parser = clap::value_parser!(u32).range(1..), help = "Metadata + arrays of the Nth chromatogram (1-based)")]
    chrom_full: Option<u32>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Cli::command();
    cmd = cmd
        .styles(cli_styles())
        .color(ColorChoice::Auto)
        .after_help(AFTER_HELP);

    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());

    if cli.version {
        println!("{VERSION}");
        return Ok(());
    }

    match cli.cmd {
        Some(Cmd::Convert(cmd)) => convert(cmd).map_err(|e| e.into()),
        Some(Cmd::Cat(cmd)) => cat(cmd).map_err(|e| e.into()),
        None => Ok(()),
    }
}

fn print_json_full<T: Serialize>(v: &T) -> Result<(), String> {
    let s = serde_json::to_string_pretty(v).map_err(|e| format!("json failed: {e}"))?;
    println!("{s}");
    Ok(())
}

fn print_json_compact<T: Serialize>(v: &T) -> Result<(), String> {
    let s = serde_json::to_string(v).map_err(|e| format!("json failed: {e}"))?;
    println!("{s}");
    Ok(())
}

fn cat(cmd: CatArgs) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("get current dir failed: {e}"))?;
    let file_path = resolve_user_path(&cwd, &cmd.file_path);
    if cmd.check {
        return check_ion_file(&file_path);
    }
    if let Some(n) = cmd.scan {
        return cat_spectrum(&file_path, n, false);
    }
    if let Some(n) = cmd.scan_full {
        return cat_spectrum(&file_path, n, true);
    }
    if let Some(n) = cmd.chrom {
        return cat_chromatogram(&file_path, n, false);
    }
    if let Some(n) = cmd.chrom_full {
        return cat_chromatogram(&file_path, n, true);
    }
    let mut mzml = read_mzml_or_ion(&file_path)?;
    if !cmd.full {
        trim_mzml_for_cat(&mut mzml);
    }
    print_json_full(&mzml)
}

fn cat_spectrum(file_path: &Path, index_1based: u32, with_arrays: bool) -> Result<(), String> {
    let index = (index_1based - 1) as usize;
    let mut spectrum = load_spectrum_at(file_path, index)?
        .ok_or_else(|| format!("spectrum {index_1based} is out of range"))?;
    if !with_arrays {
        spectrum.binary_data_array_list = None;
    }
    print_json_compact(&spectrum)
}

fn cat_chromatogram(file_path: &Path, index_1based: u32, with_arrays: bool) -> Result<(), String> {
    let index = (index_1based - 1) as usize;
    let mut chromatogram = load_chromatogram_at(file_path, index)?
        .ok_or_else(|| format!("chromatogram {index_1based} is out of range"))?;
    if !with_arrays {
        chromatogram.binary_data_array_list = None;
    }
    print_json_compact(&chromatogram)
}

fn load_spectrum_at(file_path: &Path, index: usize) -> Result<Option<Spectrum>, String> {
    match file_ext_lower(file_path).as_str() {
        "ion" => {
            let mut ion = IonReader::open_file(file_path, ReadOptions::default())
                .map_err(|e| format!("IonReader::open_file failed: {e}"))?;
            ion.spectrum(index)
                .map_err(|e| format!("get_spectrum failed: {e}"))
        }
        "mzml" => {
            let bytes = fs::read(file_path).map_err(|e| format!("read failed: {e}"))?;
            let mut mzml = parse_mzml(&bytes).map_err(|e| format!("parse_mzml failed: {e}"))?;
            Ok(mzml.run.spectrum_list.as_mut().and_then(|l| {
                (index < l.spectra.len()).then(|| std::mem::take(&mut l.spectra[index]))
            }))
        }
        other => Err(format!("unsupported file extension: {other:?}")),
    }
}

fn load_chromatogram_at(file_path: &Path, index: usize) -> Result<Option<Chromatogram>, String> {
    match file_ext_lower(file_path).as_str() {
        "ion" => {
            let mut ion = IonReader::open_file(file_path, ReadOptions::default())
                .map_err(|e| format!("IonReader::open_file failed: {e}"))?;
            ion.chromatogram(index)
                .map_err(|e| format!("get_chromatogram failed: {e}"))
        }
        "mzml" => {
            let bytes = fs::read(file_path).map_err(|e| format!("read failed: {e}"))?;
            let mut mzml = parse_mzml(&bytes).map_err(|e| format!("parse_mzml failed: {e}"))?;
            Ok(mzml.run.chromatogram_list.as_mut().and_then(|l| {
                (index < l.chromatograms.len()).then(|| std::mem::take(&mut l.chromatograms[index]))
            }))
        }
        other => Err(format!("unsupported file extension: {other:?}")),
    }
}

fn file_ext_lower(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn out_name_for_mzml_file(path: &Path, out_ext: &str) -> Option<String> {
    if file_ext_lower(path) != "mzml" {
        return None;
    }
    let stem = path.file_stem()?.to_string_lossy();
    Some(format!("{stem}.{out_ext}"))
}

fn out_name_for_bin_file_as_mzml(path: &Path) -> Option<String> {
    let ext = file_ext_lower(path);
    if ext != "ion" {
        return None;
    }
    let stem = path.file_stem()?.to_string_lossy();
    Some(format!("{stem}.mzML"))
}

fn read_mzml_or_ion(file_path: &Path) -> Result<MzML, String> {
    let bytes = fs::read(file_path).map_err(|e| format!("read failed: {e}"))?;
    let ext = file_ext_lower(file_path);

    if ext == "ion" {
        let ion = IonReader::open_file(file_path, ReadOptions::default())
            .map_err(|e| format!("IonReader::open_file failed: {e}"))?;
        return ion.metadata().map_err(|e| format!("metadata failed: {e}"));
    }
    if ext == "mzml" {
        return parse_mzml(&bytes).map_err(|e| format!("parse_mzml failed: {e}"));
    }

    Err(format!(
        "unsupported file extension: {ext:?} (expected .mzML or .ion)"
    ))
}

fn write_mzml_as_ion(
    input_path: &Path,
    output_path: &Path,
    config: WriteOptions,
) -> Result<(), String> {
    sweep_orphans(output_path)?;
    let temp_output = TempOutput::new(output_path)?;
    let mut input_reader = MzmlReader::open(input_path).map_err(|error| error.to_string())?;
    {
        let mut output_file =
            FileWriter::open_path(temp_output.path()).map_err(|error| error.to_string())?;
        let mut ion_writer =
            IonWriter::create(&mut output_file, config).map_err(|error| error.to_string())?;
        ion_writer
            .write_stream(&mut input_reader)
            .map_err(|error| error.to_string())?;
        drop(ion_writer);
        output_file.flush().map_err(|error| error.to_string())?;
    }
    temp_output.move_to(output_path)
}

#[derive(Debug, Clone)]
enum FilterExpr {
    Leaf(String),
    And(Vec<FilterExpr>),
    Or(Vec<FilterExpr>),
}

impl FilterExpr {
    fn matches(&self, name_lower: &str) -> bool {
        match self {
            Self::Leaf(pat) => name_lower.contains(pat),
            Self::And(exprs) => exprs.iter().all(|e| e.matches(name_lower)),
            Self::Or(exprs) => exprs.iter().any(|e| e.matches(name_lower)),
        }
    }

    fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("Filter pattern cannot be empty".to_string());
        }
        let or_parts: Vec<&str> = input.split('|').collect();
        if or_parts.len() > 1 {
            let exprs: Result<Vec<Self>, String> =
                or_parts.into_iter().map(Self::parse_and).collect();
            return Ok(Self::Or(exprs?));
        }
        Self::parse_and(input)
    }

    fn parse_and(input: &str) -> Result<Self, String> {
        let and_parts: Vec<&str> = input.split('&').collect();
        if and_parts.len() > 1 {
            let exprs: Vec<Self> = and_parts
                .into_iter()
                .map(|s| Self::Leaf(s.trim().to_lowercase()))
                .collect();
            return Ok(Self::And(exprs));
        }
        Ok(Self::Leaf(input.trim().to_lowercase()))
    }
}

type NameFilter = Box<dyn Fn(&str) -> bool>;

fn build_name_filter(
    pattern: Option<&str>,
    pattern_exact: Option<&str>,
    regex: Option<&str>,
) -> Result<Option<NameFilter>, String> {
    let tree = if let Some(p) = pattern {
        Some(FilterExpr::parse(p)?)
    } else {
        None
    };

    let exact = pattern_exact.map(str::to_string);

    let re = if let Some(r) = regex {
        Some(Regex::new(r).map_err(|e| format!("invalid regex: {e}"))?)
    } else {
        None
    };

    if tree.is_none() && exact.is_none() && re.is_none() {
        return Ok(None);
    }

    Ok(Some(Box::new(move |name: &str| {
        if let Some(needle) = &exact
            && name.contains(needle)
        {
            return true;
        }

        if let Some(r) = &re
            && r.is_match(name)
        {
            return true;
        }

        if let Some(t) = &tree {
            let name_lower = name.to_lowercase();
            if t.matches(&name_lower) {
                return true;
            }
        }

        false
    })))
}

fn collect_files_with_exts(
    input_root: &Path,
    exts: &[&str],
    name_filter: Option<&dyn Fn(&str) -> bool>,
) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let mut stack = vec![input_root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).map_err(|e| format!("read dir failed: {e}"))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("read dir entry failed: {e}"))?;
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if !p.is_file() {
                continue;
            }
            let ext = file_ext_lower(&p);
            if !exts.iter().any(|want| ext == *want) {
                continue;
            }
            if let Some(f) = name_filter {
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if !f(name) {
                    continue;
                }
            }
            out.push(p);
        }
    }

    out.sort();
    Ok(out)
}

fn size_to_bytes(value: f64, unit_bytes: f64, flag: &str) -> Result<usize, String> {
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{flag} must be a positive finite number"));
    }
    let bytes = value * unit_bytes;
    if bytes < 1.0 {
        return Err(format!("{flag} is too small; it rounds to zero bytes"));
    }
    if bytes > usize::MAX as f64 {
        return Err(format!("{flag} is too large"));
    }
    Ok(bytes as usize)
}

fn convert(cmd: ConvertArgs) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("get current dir failed: {e}"))?;

    let input_root = resolve_user_path(&cwd, &cmd.input_path);
    let output_root = match &cmd.output_path {
        Some(path) => resolve_user_path(&cwd, path),
        None if cmd.update => input_root.clone(),
        None => return Err("--output-path is required".to_string()),
    };

    fs::create_dir_all(&output_root).map_err(|e| format!("create output dir failed: {e}"))?;

    let filter = build_name_filter(
        cmd.pattern.as_deref(),
        cmd.pattern_exact.as_deref(),
        cmd.regex.as_deref(),
    )?;

    const MB: f64 = 1024.0 * 1024.0;

    let block_size = size_to_bytes(cmd.block_size_mb, MB, "--block-size")?;

    let cores = match cmd.cores {
        0 => std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        n => n as usize,
    };
    let pool = ThreadPoolBuilder::new()
        .num_threads(cores)
        .build()
        .map_err(|e| format!("rayon thread pool init failed: {e}"))?;

    let encoding = if cmd.many_files {
        Encoding::Sequential
    } else {
        Encoding::Parallel
    };

    if cmd.update {
        return legacy::run_update(
            &input_root,
            &output_root,
            filter.as_deref(),
            cmd.dry_run,
            &pool,
            matches!(encoding, Encoding::Sequential),
        );
    }

    let t_all = Instant::now();

    let benchmark_decode = cmd.which.benchmark_decode;

    let default_mzml_to_ion = !cmd.which.mzml_to_ion && !cmd.which.ion_to_mzml && !benchmark_decode;

    let mzml_to_ion = cmd.which.mzml_to_ion || default_mzml_to_ion;
    let ion_to_mzml = cmd.which.ion_to_mzml;

    let print_lock = Arc::new(Mutex::new(()));
    let done = Arc::new(AtomicUsize::new(0));
    let ok = Arc::new(AtomicU32::new(0));
    let failed = Arc::new(AtomicU32::new(0));
    let skipped = Arc::new(AtomicU32::new(0));
    let fixed_bad_total = Arc::new(AtomicU32::new(0));
    let had_failed = Arc::new(AtomicBool::new(false));

    if mzml_to_ion {
        let out_ext = "ion";
        let f32_compress = cmd.force_f32;

        let files = collect_files_with_exts(&input_root, &["mzml"], filter.as_deref())?;
        if files.is_empty() {
            return Err(format!(
                "no matching .mzML files found under {}",
                input_root.display()
            ));
        }

        let total = files.len();

        let convert_mzml_to_ion = |in_path: &PathBuf| {
            let rel = match in_path.strip_prefix(&input_root) {
                Ok(v) => v,
                Err(_) => {
                    had_failed.store(true, Ordering::Relaxed);
                    failed.fetch_add(1, Ordering::Relaxed);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let name = basename(in_path);
                    let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                    eprintln!(
                        "{ANSI_RED}[error]{ANSI_RESET} [{}/{}] {}: cannot make relative path",
                        n, total, name
                    );
                    let _ = stderr().flush();
                    return;
                }
            };

            let out_name = match out_name_for_mzml_file(in_path, out_ext) {
                Some(v) => v,
                None => {
                    skipped.fetch_add(1, Ordering::Relaxed);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let name = basename(in_path);
                    let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                    println!(
                        "{ANSI_YELLOW}[skipped]{ANSI_RESET} [{}/{}] {}",
                        n, total, name
                    );
                    let _ = stdout().flush();
                    return;
                }
            };

            let parent_rel = rel.parent().unwrap_or_else(|| Path::new(""));
            let out_dir = output_root.join(parent_rel);
            let out_path = out_dir.join(out_name);

            let mut fixed_bad = false;
            if !cmd.overwrite
                && let Ok(m) = fs::metadata(&out_path)
                && m.is_file()
            {
                let out_len = m.len();
                if out_len > 0 && has_valid_trailer(&out_path, out_len) {
                    skipped.fetch_add(1, Ordering::Relaxed);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;

                    let in_mb = fs::metadata(in_path)
                        .map(|m| m.len() as f64 / MB)
                        .unwrap_or(0.0);
                    let out_mb = out_len as f64 / MB;

                    let name = basename(&out_path);
                    let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                    println!(
                        "{ANSI_YELLOW}[skipped]{ANSI_RESET} [{}/{}] {}  input={:.2} MB, output={:.2} MB",
                        n, total, name, in_mb, out_mb
                    );
                    let _ = stdout().flush();
                    return;
                }
                fixed_bad = true;
            }

            if let Err(e) = fs::create_dir_all(&out_dir) {
                had_failed.store(true, Ordering::Relaxed);
                failed.fetch_add(1, Ordering::Relaxed);
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                let name = basename(&out_dir);
                let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                eprintln!(
                    "{ANSI_RED}[error]{ANSI_RESET} [{}/{}] {}: create output dir failed: {e}",
                    n, total, name
                );
                let _ = stderr().flush();
                return;
            }

            let t0 = Instant::now();

            let in_mb = fs::metadata(in_path)
                .map(|m| m.len() as f64 / MB)
                .unwrap_or(0.0);

            let config = WriteOptions {
                compression_level: cmd.compression_level,
                force_f32: f32_compress,
                block_size,
                parallel: matches!(encoding, Encoding::Parallel),
                section_storage: cmd.section_storage.storage(),
                mz_window: cmd.mz_window,
            };
            if let Err(e) = write_mzml_as_ion(in_path, &out_path, config) {
                had_failed.store(true, Ordering::Relaxed);
                failed.fetch_add(1, Ordering::Relaxed);
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                let name = basename(&out_path);
                let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                eprintln!(
                    "{ANSI_RED}[error]{ANSI_RESET} [{}/{}] {}: encode failed: {e}",
                    n, total, name
                );
                let _ = stderr().flush();
                return;
            }

            let out_mb = fs::metadata(&out_path)
                .map(|m| m.len() as f64 / MB)
                .unwrap_or(0.0);

            ok.fetch_add(1, Ordering::Relaxed);
            if fixed_bad {
                fixed_bad_total.fetch_add(1, Ordering::Relaxed);
            }
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;

            let elapsed_s = t0.elapsed().as_secs_f64();

            let (tag, color) = if fixed_bad {
                ("[fixed]", ANSI_BLUE)
            } else {
                ("[ok]", ANSI_GREEN)
            };

            let name = basename(&out_path);

            {
                let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                println!(
                    "{color}{tag}{ANSI_RESET} [{}/{}] output: {}  input={:.2} MB, output={:.2} MB, time={:.3}s",
                    n, total, name, in_mb, out_mb, elapsed_s
                );
                let _ = stdout().flush();
            }
        };

        pool.install(|| match encoding {
            Encoding::Sequential => files.par_iter().for_each(convert_mzml_to_ion),
            Encoding::Parallel => files.iter().for_each(convert_mzml_to_ion),
        });

        let ok = ok.load(Ordering::Relaxed);
        let failed = failed.load(Ordering::Relaxed);
        let skipped = skipped.load(Ordering::Relaxed);
        let fixed_bad = fixed_bad_total.load(Ordering::Relaxed);

        let d = t_all.elapsed();
        let total_secs = d.as_secs();
        let h = total_secs / 3600;
        let m = (total_secs % 3600) / 60;
        let s = total_secs % 60;

        println!(
            "ok={ok} failed={failed} skipped={skipped} fixed={fixed_bad} total_time={:02}:{:02}:{:02}",
            h, m, s
        );

        if had_failed.load(Ordering::Relaxed) {
            return Err("some files failed".to_string());
        }
        return Ok(());
    }

    if ion_to_mzml {
        let files = collect_files_with_exts(&input_root, &["ion"], filter.as_deref())?;
        if files.is_empty() {
            return Err(format!(
                "no matching .ion files found under {}",
                input_root.display()
            ));
        }

        let total = files.len();

        let convert_ion_to_mzml = |in_path: &PathBuf| {
            let rel = match in_path.strip_prefix(&input_root) {
                Ok(v) => v,
                Err(_) => {
                    had_failed.store(true, Ordering::Relaxed);
                    failed.fetch_add(1, Ordering::Relaxed);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let name = basename(in_path);
                    let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                    eprintln!(
                        "{ANSI_RED}[error]{ANSI_RESET} [{}/{}] {}: cannot make relative path",
                        n, total, name
                    );
                    let _ = stderr().flush();
                    return;
                }
            };

            let out_name = match out_name_for_bin_file_as_mzml(in_path) {
                Some(v) => v,
                None => {
                    skipped.fetch_add(1, Ordering::Relaxed);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let name = basename(in_path);
                    let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                    println!(
                        "{ANSI_YELLOW}[skipped]{ANSI_RESET} [{}/{}] {}",
                        n, total, name
                    );
                    let _ = stdout().flush();
                    return;
                }
            };

            let parent_rel = rel.parent().unwrap_or_else(|| Path::new(""));
            let out_dir = output_root.join(parent_rel);
            let out_path = out_dir.join(out_name);

            if !cmd.overwrite
                && let Ok(m) = fs::metadata(&out_path)
                && m.is_file()
                && m.len() > 0
            {
                skipped.fetch_add(1, Ordering::Relaxed);
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;

                let in_mb = fs::metadata(in_path)
                    .map(|m| m.len() as f64 / MB)
                    .unwrap_or(0.0);
                let out_mb = m.len() as f64 / MB;

                let name = basename(&out_path);
                let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                println!(
                    "{ANSI_YELLOW}[skipped]{ANSI_RESET} [{}/{}] {}  input={:.2} MB, output={:.2} MB",
                    n, total, name, in_mb, out_mb
                );
                let _ = stdout().flush();
                return;
            }

            if let Err(e) = fs::create_dir_all(&out_dir) {
                had_failed.store(true, Ordering::Relaxed);
                failed.fetch_add(1, Ordering::Relaxed);
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                let name = basename(&out_dir);
                let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                eprintln!(
                    "{ANSI_RED}[error]{ANSI_RESET} [{}/{}] {}: create output dir failed: {e}",
                    n, total, name
                );
                let _ = stderr().flush();
                return;
            }

            let t0 = Instant::now();

            let in_mb = fs::metadata(in_path)
                .map(|m| m.len() as f64 / MB)
                .unwrap_or(0.0);

            let mut ion = match IonReader::open_file(
                in_path,
                ReadOptions {
                    parallel: matches!(encoding, Encoding::Parallel),
                    ..ReadOptions::default()
                },
            ) {
                Ok(v) => v,
                Err(e) => {
                    had_failed.store(true, Ordering::Relaxed);
                    failed.fetch_add(1, Ordering::Relaxed);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let name = basename(in_path);
                    let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                    eprintln!(
                        "{ANSI_RED}[error]{ANSI_RESET} [{}/{}] {}: IonReader::open_file failed: {e}",
                        n, total, name
                    );
                    let _ = stderr().flush();
                    return;
                }
            };

            let mzml = match ion.to_mzml() {
                Ok(v) => v,
                Err(e) => {
                    had_failed.store(true, Ordering::Relaxed);
                    failed.fetch_add(1, Ordering::Relaxed);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let name = basename(in_path);
                    let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                    eprintln!(
                        "{ANSI_RED}[error]{ANSI_RESET} [{}/{}] {}: to_mzml failed: {e}",
                        n, total, name
                    );
                    let _ = stderr().flush();
                    return;
                }
            };

            let xml = match bin_to_mzml(&mzml) {
                Ok(v) => v,
                Err(e) => {
                    had_failed.store(true, Ordering::Relaxed);
                    failed.fetch_add(1, Ordering::Relaxed);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let name = basename(in_path);
                    let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                    eprintln!(
                        "{ANSI_RED}[error]{ANSI_RESET} [{}/{}] {}: bin_to_mzml failed: {e}",
                        n, total, name
                    );
                    let _ = stderr().flush();
                    return;
                }
            };
            drop(mzml);

            if let Err(e) = fs::write(&out_path, &xml) {
                had_failed.store(true, Ordering::Relaxed);
                failed.fetch_add(1, Ordering::Relaxed);
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                let name = basename(&out_path);
                let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                eprintln!(
                    "{ANSI_RED}[error]{ANSI_RESET} [{}/{}] {}: write failed: {e}",
                    n, total, name
                );
                let _ = stderr().flush();
                return;
            }
            let out_mb = xml.len() as f64 / MB;
            drop(xml);

            ok.fetch_add(1, Ordering::Relaxed);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;

            let elapsed_s = t0.elapsed().as_secs_f64();

            let name = basename(&out_path);

            let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
            println!(
                "{ANSI_GREEN}[ok]{ANSI_RESET} [{}/{}] output: {}  input={:.2} MB, output={:.2} MB, time={:.3}s",
                n, total, name, in_mb, out_mb, elapsed_s
            );
            let _ = stdout().flush();
        };

        pool.install(|| match encoding {
            Encoding::Sequential => files.par_iter().for_each(convert_ion_to_mzml),
            Encoding::Parallel => files.iter().for_each(convert_ion_to_mzml),
        });

        let ok = ok.load(Ordering::Relaxed);
        let failed = failed.load(Ordering::Relaxed);
        let skipped = skipped.load(Ordering::Relaxed);

        let d = t_all.elapsed();
        let total_secs = d.as_secs();
        let h = total_secs / 3600;
        let m = (total_secs % 3600) / 60;
        let s = total_secs % 60;

        println!(
            "ok={ok} failed={failed} skipped={skipped} total_time={:02}:{:02}:{:02}",
            h, m, s
        );

        if had_failed.load(Ordering::Relaxed) {
            return Err("some files failed".to_string());
        }
        return Ok(());
    }

    if benchmark_decode {
        let files = collect_files_with_exts(&input_root, &["ion"], filter.as_deref())?;
        if files.is_empty() {
            return Err(format!(
                "no matching .ion files found under {}",
                input_root.display()
            ));
        }
        let total = files.len();
        let benchmark_one = |in_path: &PathBuf| {
            let name = basename(in_path);
            let t0 = Instant::now();
            match IonReader::open_file(
                in_path,
                ReadOptions {
                    parallel: matches!(encoding, Encoding::Parallel),
                    ..ReadOptions::default()
                },
            ) {
                Ok(mut ion) => {
                    let count = ion.spectrum_count() as usize;
                    let mut buf = Vec::new();
                    for i in 0..count {
                        if let Some(addresses) = ion.spectrum_array_addresses(i) {
                            for address in addresses {
                                let _ = ion.read_spectrum_values(&address, &mut buf);
                            }
                        }
                    }
                    let elapsed_s = t0.elapsed().as_secs_f64();
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let in_mb = fs::metadata(in_path)
                        .map(|m| m.len() as f64 / MB)
                        .unwrap_or(0.0);
                    let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                    println!(
                        "{ANSI_GREEN}[ok]{ANSI_RESET} [{}/{}] {}  {:.2} MB  {:.3}s",
                        n, total, name, in_mb, elapsed_s
                    );
                    let _ = stdout().flush();
                }
                Err(e) => {
                    had_failed.store(true, Ordering::Relaxed);
                    failed.fetch_add(1, Ordering::Relaxed);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let _g = print_lock.lock().unwrap_or_else(|e| e.into_inner());
                    eprintln!(
                        "{ANSI_RED}[error]{ANSI_RESET} [{}/{}] {}: {e}",
                        n, total, name
                    );
                    let _ = stderr().flush();
                }
            }
        };
        pool.install(|| match encoding {
            Encoding::Sequential => files.par_iter().for_each(benchmark_one),
            Encoding::Parallel => files.iter().for_each(benchmark_one),
        });
        if had_failed.load(Ordering::Relaxed) {
            return Err("some files failed".to_string());
        }
        return Ok(());
    }

    Err("no convert mode selected".to_string())
}

fn resolve_user_path(cwd: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

#[inline]
fn has_valid_trailer(path: &Path, file_len: u64) -> bool {
    if file_len < FILE_TRAILER.len() as u64 {
        return false;
    }

    let mut f = match fs::File::open(path) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let back = -(FILE_TRAILER.len() as i64);
    if f.seek(SeekFrom::End(back)).is_err() {
        return false;
    }

    let mut tail = [0u8; 8];
    if f.read_exact(&mut tail).is_err() {
        return false;
    }

    tail == FILE_TRAILER
}

#[inline]
fn basename(p: &Path) -> std::borrow::Cow<'_, str> {
    p.file_name().unwrap_or(p.as_os_str()).to_string_lossy()
}

#[inline]
fn trim_mzml_for_cat(mzml: &mut MzML) {
    if let Some(s) = mzml.run.spectrum_list.as_mut() {
        s.spectra.clear();
    }
    if let Some(c) = mzml.run.chromatogram_list.as_mut() {
        c.chromatograms.clear();
    }
}
