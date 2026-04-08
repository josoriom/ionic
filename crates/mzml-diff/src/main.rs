mod cli;
mod diff;
mod report;
mod tables;
mod xml;

use std::fs::File;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use rayon::join;

use cli::{Args, ColorMode};
use diff::structural::{diff_counts, sort_deltas};
use report::render::ReportCtx;
use xml::CanonIndex;

fn main() -> Result<()> {
    let args = Args::parse();

    let all_start = Instant::now();
    let mode = args.mode;
    let inline_text_max = args.inline_text_max;

    let ((left_index, left_elapsed), (right_index, right_elapsed)) = join(
        || timed_build_index(&args.left, inline_text_max, mode),
        || timed_build_index(&args.right, inline_text_max, mode),
    );

    let left_index = left_index?;
    let right_index = right_index?;
    let total_elapsed = all_start.elapsed();

    // Compute deltas.
    let mut node_deltas = diff_counts(&left_index.nodes, &right_index.nodes);
    let mut attr_deltas = diff_counts(&left_index.attr_counts, &right_index.attr_counts);
    let mut text_short_deltas =
        diff_counts(&left_index.text_short_counts, &right_index.text_short_counts);
    let mut text_large_deltas =
        diff_counts(&left_index.text_large_counts, &right_index.text_large_counts);

    sort_deltas(&mut node_deltas, |k| &k.path, |_| "");
    sort_deltas(&mut attr_deltas, |k| &k.path, |k| &k.name);
    sort_deltas(&mut text_short_deltas, |k| &k.path, |_| "");
    sort_deltas(&mut text_large_deltas, |k| &k.path, |_| "");

    // Resolve color mode.
    let is_tty = std::io::stdout().is_terminal();
    let color_enabled = match args.color {
        ColorMode::Auto => is_tty,
        ColorMode::Always => true,
        ColorMode::Never => false,
    };

    // Render terminal report.
    let ctx = ReportCtx {
        args: &args,
        left: &left_index,
        right: &right_index,
        node_deltas: &node_deltas,
        attr_deltas: &attr_deltas,
        text_short_deltas: &text_short_deltas,
        text_large_deltas: &text_large_deltas,
        left_elapsed,
        right_elapsed,
        total_elapsed,
        limit: Some(args.top),
        color: color_enabled,
    };
    let rendered = ctx.render();

    // Output — either through pager or direct.
    if args.pager && is_tty {
        output_through_pager(&rendered);
    } else {
        print!("{rendered}");
    }

    // Optional file report (no colour, no limit).
    if let Some(ref path) = args.report {
        let file_ctx = ReportCtx {
            args: &args,
            left: &left_index,
            right: &right_index,
            node_deltas: &node_deltas,
            attr_deltas: &attr_deltas,
            text_short_deltas: &text_short_deltas,
            text_large_deltas: &text_large_deltas,
            left_elapsed,
            right_elapsed,
            total_elapsed,
            limit: None,
            color: false,
        };
        let plain_report = file_ctx.render();
        let mut f = File::create(path)
            .with_context(|| format!("cannot create report file {}", path.display()))?;
        f.write_all(plain_report.as_bytes())
            .with_context(|| format!("cannot write report file {}", path.display()))?;
    }

    // Exit with non-zero status if diffs found. The report is already printed,
    // so we use process::exit instead of bail to avoid anyhow's "Error: ..."
    // prefix cluttering the output.
    let has_diffs = !node_deltas.is_empty()
        || !attr_deltas.is_empty()
        || !text_short_deltas.is_empty()
        || !text_large_deltas.is_empty();

    if has_diffs {
        std::process::exit(1);
    }

    Ok(())
}

/// Pipe output through `less -R`. Falls back to direct print if less is
/// unavailable or the pipe fails.
fn output_through_pager(text: &str) {
    let child = Command::new("less")
        .args(["-R", "-F", "-X"])
        .stdin(Stdio::piped())
        .spawn();

    match child {
        Ok(mut proc) => {
            if let Some(mut stdin) = proc.stdin.take() {
                // Ignore broken-pipe errors (user quit less early).
                let _ = stdin.write_all(text.as_bytes());
                drop(stdin);
            }
            let _ = proc.wait();
        }
        Err(_) => {
            // less not found — fall back to direct output.
            print!("{text}");
        }
    }
}

fn timed_build_index(
    path: &Path,
    inline_text_max: usize,
    mode: cli::DiffMode,
) -> (Result<CanonIndex>, std::time::Duration) {
    let start = Instant::now();
    let out = xml::build_index(path, inline_text_max, mode);
    (out, start.elapsed())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::cli::DiffMode;
    use crate::diff::structural::diff_counts;
    use crate::xml::build_index;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../parser/data/mzml")
            .join(name)
    }

    #[test]
    fn identical_fixture_has_no_differences() {
        let path = fixture("tiny1.mzML0.99.0.mzML");
        let left = build_index(&path, 256, DiffMode::Canonical).expect("left parse");
        let right = build_index(&path, 256, DiffMode::Canonical).expect("right parse");

        assert!(diff_counts(&left.nodes, &right.nodes).is_empty());
        assert!(diff_counts(&left.attr_counts, &right.attr_counts).is_empty());
        assert!(diff_counts(&left.text_short_counts, &right.text_short_counts).is_empty());
        assert!(diff_counts(&left.text_large_counts, &right.text_large_counts).is_empty());
    }

    #[test]
    fn different_fixture_detects_differences() {
        let left = build_index(&fixture("tiny1.mzML0.99.0.mzML"), 256, DiffMode::Canonical)
            .expect("left parse");
        let right = build_index(&fixture("tiny1.mzML0.99.1.mzML"), 256, DiffMode::Canonical)
            .expect("right parse");

        let any_diff = !diff_counts(&left.nodes, &right.nodes).is_empty()
            || !diff_counts(&left.attr_counts, &right.attr_counts).is_empty()
            || !diff_counts(&left.text_short_counts, &right.text_short_counts).is_empty()
            || !diff_counts(&left.text_large_counts, &right.text_large_counts).is_empty();

        assert!(any_diff);
    }
}
