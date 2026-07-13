use std::{
    fs,
    io::{IsTerminal, Read, Seek, SeekFrom, Write, stderr, stdout},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
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

mod utilities;

use utilities::{TempOutput, check_ion_file, sweep_orphans};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const MB: f64 = 1024.0 * 1024.0;

static COLOR_ENABLED: OnceLock<bool> = OnceLock::new();

fn color_enabled() -> bool {
    *COLOR_ENABLED.get_or_init(|| {
        std::env::var_os("NO_COLOR").is_none() && stdout().is_terminal() && stderr().is_terminal()
    })
}

fn ansi(code: &'static str) -> &'static str {
    if color_enabled() { code } else { "" }
}

fn ansi_reset() -> &'static str {
    ansi("\x1b[0m")
}

fn ansi_green() -> &'static str {
    ansi("\x1b[1;32m")
}

fn ansi_yellow() -> &'static str {
    ansi("\x1b[1;33m")
}

fn ansi_red() -> &'static str {
    ansi("\x1b[1;31m")
}

fn ansi_blue() -> &'static str {
    ansi("\x1b[1;34m")
}

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
    #[arg(short = 'v', long = "version", action = ArgAction::SetTrue, help = "Print the version")]
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
            .args(["mzml_to_ion", "ion_to_mzml", "benchmark_decode"])
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

    #[arg(
        long = "mz-window",
        default_value_t = 250.0,
        value_name = "DA",
        help = "m/z split width in Da (smaller = read less)"
    )]
    mz_window: f64,

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

    #[arg(
        long = "benchmark-decode",
        help = "Benchmark decode speed by reading every spectrum's arrays"
    )]
    benchmark_decode: bool,
}

#[derive(Clone, Copy)]
enum Encoding {
    WithinFileParallel,
    FileLevelParallel,
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

fn main() -> std::process::ExitCode {
    color_enabled();

    let mut cmd = Cli::command();
    cmd = cmd
        .styles(cli_styles())
        .color(ColorChoice::Auto)
        .after_help(AFTER_HELP);

    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());

    if cli.version {
        println!("{VERSION}");
        return std::process::ExitCode::SUCCESS;
    }

    let result = match cli.cmd {
        Some(Cmd::Convert(cmd)) => convert(cmd),
        Some(Cmd::Cat(cmd)) => cat(cmd),
        None => Ok(()),
    };

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::ExitCode::FAILURE
        }
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
        if file_ext_lower(&file_path) != "ion" {
            return Err(format!("--check expects a .ion file, got {file_path:?}"));
        }
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
    let ext = file_ext_lower(file_path);

    if ext == "ion" {
        let ion = IonReader::open_file(file_path, ReadOptions::default())
            .map_err(|e| format!("IonReader::open_file failed: {e}"))?;
        return ion.metadata().map_err(|e| format!("metadata failed: {e}"));
    }
    if ext == "mzml" {
        let bytes = fs::read(file_path).map_err(|e| format!("read failed: {e}"))?;
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
            && name == needle.as_str()
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

fn accepts_file(path: &Path, exts: &[&str], name_filter: Option<&dyn Fn(&str) -> bool>) -> bool {
    let ext = file_ext_lower(path);
    if !exts.iter().any(|want| ext == *want) {
        return false;
    }
    if let Some(f) = name_filter {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !f(name) {
            return false;
        }
    }
    true
}

fn collect_files_with_exts(
    input_root: &Path,
    exts: &[&str],
    name_filter: Option<&dyn Fn(&str) -> bool>,
) -> Result<Vec<PathBuf>, String> {
    if input_root.is_file() {
        let mut out = Vec::new();
        if accepts_file(input_root, exts, name_filter) {
            out.push(input_root.to_path_buf());
        }
        return Ok(out);
    }

    let mut out = Vec::new();
    let mut stack = vec![input_root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).map_err(|e| format!("read dir failed: {e}"))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("read dir entry failed: {e}"))?;
            let file_type = match entry.file_type() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let p = entry.path();
            if file_type.is_dir() {
                stack.push(p);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            if accepts_file(&p, exts, name_filter) {
                out.push(p);
            }
        }
    }

    out.sort();
    Ok(out)
}

fn validate_positive_finite(value: f64, flag: &str) -> Result<(), String> {
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{flag} must be a positive finite number"));
    }
    Ok(())
}

fn size_to_bytes(value: f64, unit_bytes: f64, flag: &str) -> Result<usize, String> {
    validate_positive_finite(value, flag)?;
    let bytes = value * unit_bytes;
    if bytes < 1.0 {
        return Err(format!("{flag} is too small; it rounds to zero bytes"));
    }
    if bytes > usize::MAX as f64 {
        return Err(format!("{flag} is too large"));
    }
    Ok(bytes as usize)
}

struct Palette {
    reset: &'static str,
    green: &'static str,
    yellow: &'static str,
    red: &'static str,
    blue: &'static str,
}

impl Palette {
    fn current() -> Self {
        Self {
            reset: ansi_reset(),
            green: ansi_green(),
            yellow: ansi_yellow(),
            red: ansi_red(),
            blue: ansi_blue(),
        }
    }
}

struct ConvertCounters {
    print_lock: Mutex<()>,
    done: AtomicUsize,
    ok: AtomicU32,
    failed: AtomicU32,
    skipped: AtomicU32,
    fixed: AtomicU32,
    had_failed: AtomicBool,
}

impl ConvertCounters {
    fn new() -> Self {
        Self {
            print_lock: Mutex::new(()),
            done: AtomicUsize::new(0),
            ok: AtomicU32::new(0),
            failed: AtomicU32::new(0),
            skipped: AtomicU32::new(0),
            fixed: AtomicU32::new(0),
            had_failed: AtomicBool::new(false),
        }
    }

    fn next_step(&self) -> usize {
        self.done.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn had_any_failure(&self) -> bool {
        self.had_failed.load(Ordering::Relaxed)
    }
}

fn print_progress_line(print_lock: &Mutex<()>, to_stderr: bool, line: &str) {
    let _guard = print_lock.lock().unwrap_or_else(|e| e.into_inner());
    if to_stderr {
        eprintln!("{line}");
        let _ = stderr().flush();
    } else {
        println!("{line}");
        let _ = stdout().flush();
    }
}

fn print_convert_totals(counters: &ConvertCounters, elapsed: std::time::Duration) {
    let ok = counters.ok.load(Ordering::Relaxed);
    let failed = counters.failed.load(Ordering::Relaxed);
    let skipped = counters.skipped.load(Ordering::Relaxed);
    let fixed = counters.fixed.load(Ordering::Relaxed);
    let total_secs = elapsed.as_secs();
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    println!(
        "ok={ok} failed={failed} skipped={skipped} fixed={fixed} total_time={h:02}:{m:02}:{s:02}"
    );
}

struct ConversionJob<'a> {
    counters: &'a ConvertCounters,
    palette: &'a Palette,
    input_root: &'a Path,
    output_root: &'a Path,
    overwrite: bool,
    total: usize,
}

impl<'a> ConversionJob<'a> {
    fn run(
        &self,
        in_path: &Path,
        derive_out_name: impl Fn(&Path) -> Option<String>,
        output_is_valid: impl Fn(&Path, u64) -> bool,
        perform: impl FnOnce(&Path, &Path) -> Result<(), String>,
    ) {
        let rel = match in_path.strip_prefix(self.input_root) {
            Ok(v) => v,
            Err(_) => {
                self.report_error(in_path, "cannot make relative path".to_string());
                return;
            }
        };

        let out_name = match derive_out_name(in_path) {
            Some(v) => v,
            None => {
                self.report_skipped_unnamed(in_path);
                return;
            }
        };

        let parent_rel = rel.parent().unwrap_or_else(|| Path::new(""));
        let out_dir = self.output_root.join(parent_rel);
        let out_path = out_dir.join(out_name);

        let mut fixing_bad_output = false;
        if !self.overwrite
            && let Ok(m) = fs::metadata(&out_path)
            && m.is_file()
        {
            let out_len = m.len();
            if out_len > 0 && output_is_valid(&out_path, out_len) {
                self.report_skipped_existing(in_path, &out_path, out_len);
                return;
            }
            fixing_bad_output = true;
        }

        if let Err(e) = fs::create_dir_all(&out_dir) {
            self.report_error(&out_dir, format!("create output dir failed: {e}"));
            return;
        }

        let t0 = Instant::now();
        let in_mb = megabytes_of(in_path);

        if let Err(message) = perform(in_path, &out_path) {
            self.report_error(&out_path, message);
            return;
        }

        let out_mb = megabytes_of(&out_path);
        self.counters.ok.fetch_add(1, Ordering::Relaxed);
        if fixing_bad_output {
            self.counters.fixed.fetch_add(1, Ordering::Relaxed);
        }
        let n = self.counters.next_step();
        let elapsed_s = t0.elapsed().as_secs_f64();

        let (tag, color) = if fixing_bad_output {
            ("[fixed]", self.palette.blue)
        } else {
            ("[ok]", self.palette.green)
        };
        let name = basename(&out_path);
        let reset = self.palette.reset;
        let total = self.total;
        let line = format!(
            "{color}{tag}{reset} [{n}/{total}] output: {name}  input={in_mb:.2} MB, output={out_mb:.2} MB, time={elapsed_s:.3}s"
        );
        print_progress_line(&self.counters.print_lock, false, &line);
    }

    fn report_error(&self, path: &Path, message: String) {
        self.counters.had_failed.store(true, Ordering::Relaxed);
        self.counters.failed.fetch_add(1, Ordering::Relaxed);
        let n = self.counters.next_step();
        let name = basename(path);
        let red = self.palette.red;
        let reset = self.palette.reset;
        let total = self.total;
        let line = format!("{red}[error]{reset} [{n}/{total}] {name}: {message}");
        print_progress_line(&self.counters.print_lock, true, &line);
    }

    fn report_skipped_unnamed(&self, in_path: &Path) {
        self.counters.skipped.fetch_add(1, Ordering::Relaxed);
        let n = self.counters.next_step();
        let name = basename(in_path);
        let yellow = self.palette.yellow;
        let reset = self.palette.reset;
        let total = self.total;
        let line = format!("{yellow}[skipped]{reset} [{n}/{total}] {name}");
        print_progress_line(&self.counters.print_lock, false, &line);
    }

    fn report_skipped_existing(&self, in_path: &Path, out_path: &Path, out_len: u64) {
        self.counters.skipped.fetch_add(1, Ordering::Relaxed);
        let n = self.counters.next_step();
        let in_mb = megabytes_of(in_path);
        let out_mb = out_len as f64 / MB;
        let name = basename(out_path);
        let yellow = self.palette.yellow;
        let reset = self.palette.reset;
        let total = self.total;
        let line = format!(
            "{yellow}[skipped]{reset} [{n}/{total}] {name}  input={in_mb:.2} MB, output={out_mb:.2} MB"
        );
        print_progress_line(&self.counters.print_lock, false, &line);
    }
}

fn convert(cmd: ConvertArgs) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("get current dir failed: {e}"))?;

    let input_root = resolve_user_path(&cwd, &cmd.input_path);
    let output_root = match &cmd.output_path {
        Some(path) => resolve_user_path(&cwd, path),
        None => return Err("--output-path is required".to_string()),
    };

    fs::create_dir_all(&output_root).map_err(|e| format!("create output dir failed: {e}"))?;

    let filter = build_name_filter(
        cmd.pattern.as_deref(),
        cmd.pattern_exact.as_deref(),
        cmd.regex.as_deref(),
    )?;

    let block_size = size_to_bytes(cmd.block_size_mb, MB, "--block-size")?;
    validate_positive_finite(cmd.mz_window, "--mz-window")?;

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
        Encoding::FileLevelParallel
    } else {
        Encoding::WithinFileParallel
    };

    let t_all = Instant::now();

    let benchmark_decode = cmd.which.benchmark_decode;

    let default_mzml_to_ion = !cmd.which.mzml_to_ion && !cmd.which.ion_to_mzml && !benchmark_decode;

    let mzml_to_ion = cmd.which.mzml_to_ion || default_mzml_to_ion;
    let ion_to_mzml = cmd.which.ion_to_mzml;

    let palette = Palette::current();
    let counters = ConvertCounters::new();

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

        let job = ConversionJob {
            counters: &counters,
            palette: &palette,
            input_root: &input_root,
            output_root: &output_root,
            overwrite: cmd.overwrite,
            total: files.len(),
        };

        let config = WriteOptions {
            compression_level: cmd.compression_level,
            force_f32: f32_compress,
            block_size,
            parallel: matches!(encoding, Encoding::WithinFileParallel),
            section_storage: cmd.section_storage.storage(),
            mz_window: cmd.mz_window,
        };

        let convert_mzml_to_ion = |in_path: &PathBuf| {
            job.run(
                in_path,
                |p| out_name_for_mzml_file(p, out_ext),
                has_valid_trailer,
                |in_path, out_path| {
                    write_mzml_as_ion(in_path, out_path, config)
                        .map_err(|e| format!("encode failed: {e}"))
                },
            );
        };

        pool.install(|| match encoding {
            Encoding::FileLevelParallel => files.par_iter().for_each(convert_mzml_to_ion),
            Encoding::WithinFileParallel => files.iter().for_each(convert_mzml_to_ion),
        });

        print_convert_totals(&counters, t_all.elapsed());

        if counters.had_any_failure() {
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

        let job = ConversionJob {
            counters: &counters,
            palette: &palette,
            input_root: &input_root,
            output_root: &output_root,
            overwrite: cmd.overwrite,
            total: files.len(),
        };

        let convert_ion_to_mzml = |in_path: &PathBuf| {
            job.run(
                in_path,
                out_name_for_bin_file_as_mzml,
                mzml_output_is_complete,
                |in_path, out_path| {
                    let mut ion = IonReader::open_file(
                        in_path,
                        ReadOptions {
                            parallel: matches!(encoding, Encoding::WithinFileParallel),
                            ..ReadOptions::default()
                        },
                    )
                    .map_err(|e| format!("IonReader::open_file failed: {e}"))?;

                    let mzml = ion.to_mzml().map_err(|e| format!("to_mzml failed: {e}"))?;

                    let xml =
                        bin_to_mzml(&mzml).map_err(|e| format!("bin_to_mzml failed: {e}"))?;
                    drop(mzml);

                    fs::write(out_path, &xml).map_err(|e| format!("write failed: {e}"))
                },
            );
        };

        pool.install(|| match encoding {
            Encoding::FileLevelParallel => files.par_iter().for_each(convert_ion_to_mzml),
            Encoding::WithinFileParallel => files.iter().for_each(convert_ion_to_mzml),
        });

        print_convert_totals(&counters, t_all.elapsed());

        if counters.had_any_failure() {
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
                    parallel: matches!(encoding, Encoding::WithinFileParallel),
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
                    let n = counters.next_step();
                    let in_mb = megabytes_of(in_path);
                    let green = palette.green;
                    let reset = palette.reset;
                    let line =
                        format!("{green}[ok]{reset} [{n}/{total}] {name}  {in_mb:.2} MB  {elapsed_s:.3}s");
                    print_progress_line(&counters.print_lock, false, &line);
                }
                Err(e) => {
                    counters.had_failed.store(true, Ordering::Relaxed);
                    counters.failed.fetch_add(1, Ordering::Relaxed);
                    let n = counters.next_step();
                    let red = palette.red;
                    let reset = palette.reset;
                    let line = format!("{red}[error]{reset} [{n}/{total}] {name}: {e}");
                    print_progress_line(&counters.print_lock, true, &line);
                }
            }
        };
        pool.install(|| match encoding {
            Encoding::FileLevelParallel => files.par_iter().for_each(benchmark_one),
            Encoding::WithinFileParallel => files.iter().for_each(benchmark_one),
        });
        if counters.had_any_failure() {
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

const MZML_CLOSING_TAG: &[u8] = b"</indexedmzML>";
const MZML_TAIL_SCAN_LEN: u64 = 64;

#[inline]
fn mzml_output_is_complete(path: &Path, file_len: u64) -> bool {
    if file_len == 0 {
        return false;
    }

    let mut f = match fs::File::open(path) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let read_len = file_len.min(MZML_TAIL_SCAN_LEN) as usize;
    let back = -(read_len as i64);
    if f.seek(SeekFrom::End(back)).is_err() {
        return false;
    }

    let mut tail = vec![0u8; read_len];
    if f.read_exact(&mut tail).is_err() {
        return false;
    }

    trim_ascii_end(&tail).ends_with(MZML_CLOSING_TAG)
}

fn trim_ascii_end(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &bytes[..end]
}

#[inline]
fn megabytes_of(path: &Path) -> f64 {
    fs::metadata(path)
        .map(|m| m.len() as f64 / MB)
        .unwrap_or(0.0)
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
