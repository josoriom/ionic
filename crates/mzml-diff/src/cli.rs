use std::path::PathBuf;

use clap::Parser;

pub const DEFAULT_TOP: usize = 80;
pub const DEFAULT_INLINE_TEXT_MAX: usize = 256;

/// Diff mode selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum DiffMode {
    /// Canonical (byte-exact) diff — original behaviour.
    #[default]
    Canonical,
    /// Semantic diff — suppresses transport noise and normalises cv values.
    Semantic,
}

impl DiffMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Semantic => "semantic",
        }
    }
}

/// Color mode selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ColorMode {
    /// Enable colors when stdout is a terminal.
    #[default]
    Auto,
    /// Always emit ANSI color codes (useful when piping to `less -R`).
    Always,
    /// Never emit ANSI color codes.
    Never,
}

#[derive(Debug, Parser)]
#[command(
    name = "mzml-diff",
    version,
    about = "mzML diff: canonical (byte-exact) or semantic (noise-filtered, cv-aware)"
)]
pub struct Args {
    #[arg(long, short = 'l')]
    pub left: PathBuf,

    #[arg(long, short = 'r')]
    pub right: PathBuf,

    #[arg(long, short = 'o')]
    pub report: Option<PathBuf>,

    #[arg(long, default_value_t = DEFAULT_TOP)]
    pub top: usize,

    #[arg(long, default_value_t = DEFAULT_INLINE_TEXT_MAX)]
    pub inline_text_max: usize,

    /// Color output: `auto` (default), `always`, or `never`.
    #[arg(long, value_enum, default_value_t = ColorMode::Auto)]
    pub color: ColorMode,

    /// Pipe output through a pager (`less -R`) for easy navigation.
    /// Off by default.
    #[arg(long)]
    pub pager: bool,

    /// Diff mode: `canonical` (default) or `semantic`.
    ///
    /// In semantic mode: transport bookkeeping attributes (count, index,
    /// arrayLength, encodedLength, dataProcessingRef, etc.) and elements
    /// (indexList, offset, fileChecksum) are suppressed, and cvParam.value is
    /// normalised for numeric CV terms (so "20" == "20.0").
    #[arg(long, value_enum, default_value_t = DiffMode::Canonical)]
    pub mode: DiffMode,
}
