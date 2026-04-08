use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use crate::cli::{Args, DiffMode};
use crate::diff::semantic::{classify_attr, classify_node, classify_text, SemKind};
use crate::diff::structural::Delta;
use crate::xml::parse::{
    AttrKey, CanonIndex, NodeKey, TextLargeKey, TextShortKey, PREVIEW_LINE_LIMIT,
};

use super::format::{
    clip, format_bytes, format_count, format_duration, hex_prefix, render_table, Paint,
};

/// Context for rendering a report. Replaces the old 12+ parameter functions.
pub struct ReportCtx<'a> {
    pub args: &'a Args,
    pub left: &'a CanonIndex,
    pub right: &'a CanonIndex,
    pub node_deltas: &'a [Delta<NodeKey>],
    pub attr_deltas: &'a [Delta<AttrKey>],
    pub text_short_deltas: &'a [Delta<TextShortKey>],
    pub text_large_deltas: &'a [Delta<TextLargeKey>],
    pub left_elapsed: Duration,
    pub right_elapsed: Duration,
    pub total_elapsed: Duration,
    pub limit: Option<usize>,
    pub color: bool,
}

impl ReportCtx<'_> {
    pub fn render(&self) -> String {
        match self.args.mode {
            DiffMode::Semantic => self.render_semantic(),
            DiffMode::Canonical => self.render_canonical(),
        }
    }

    fn total_diffs(&self) -> usize {
        self.node_deltas.len()
            + self.attr_deltas.len()
            + self.text_short_deltas.len()
            + self.text_large_deltas.len()
    }

    // ── Shared sections ─────────────────────────────────────────────────

    /// Title line + verdict + file paths.
    fn render_title(&self, p: &Paint, out: &mut String, categories: usize) {
        let total = self.total_diffs();
        let mode = self.args.mode.label();

        let _ = writeln!(
            out,
            "{}",
            p.bold(&format!("mzml-diff  {mode}  order-insensitive"))
        );
        out.push('\n');

        // Verdict — first thing the user sees.
        if total == 0 {
            let _ = writeln!(
                out,
                "  {}  No {} differences detected.",
                p.pass(" PASS "),
                mode
            );
        } else {
            let _ = writeln!(
                out,
                "  {}  {} across {} {}",
                p.fail(" FAIL "),
                p.fail(&format!("{} differences", format_count(total as u64))),
                categories,
                if categories == 1 {
                    "category"
                } else {
                    "categories"
                }
            );
        }
        out.push('\n');

        // File paths.
        let _ = writeln!(out, "  left:   {}", self.args.left.display());
        let _ = writeln!(out, "  right:  {}", self.args.right.display());
        out.push('\n');
    }

    /// Parsing stats block — aligned columns with formatted numbers.
    fn render_stats(&self, p: &Paint, out: &mut String) {
        let _ = writeln!(out, "  {}", p.dim("Parsing"));
        let _ = writeln!(
            out,
            "    left   {:>9} nodes  {:>9} attrs  {:>7} texts  {:>10}    {}",
            format_count(self.left.totals.nodes),
            format_count(self.left.totals.attrs),
            format_count(self.left.totals.text_nodes),
            format_bytes(self.left.totals.text_bytes),
            p.dim(&format_duration(self.left_elapsed))
        );
        let _ = writeln!(
            out,
            "    right  {:>9} nodes  {:>9} attrs  {:>7} texts  {:>10}    {}",
            format_count(self.right.totals.nodes),
            format_count(self.right.totals.attrs),
            format_count(self.right.totals.text_nodes),
            format_bytes(self.right.totals.text_bytes),
            p.dim(&format_duration(self.right_elapsed))
        );
        let _ = writeln!(
            out,
            "    wall   {}",
            p.dim(&format!(
                "{} (parallel)",
                format_duration(self.total_elapsed)
            ))
        );
        out.push('\n');
    }

    /// How many distinct canonical diff categories have at least one entry.
    fn canonical_category_count(&self) -> usize {
        let mut n = 0;
        if !self.node_deltas.is_empty() {
            n += 1;
        }
        if !self.attr_deltas.is_empty() {
            n += 1;
        }
        if !self.text_short_deltas.is_empty() {
            n += 1;
        }
        if !self.text_large_deltas.is_empty() {
            n += 1;
        }
        n
    }

    // ── Canonical mode ──────────────────────────────────────────────────

    fn render_canonical(&self) -> String {
        let p = Paint::new(self.color);
        let mut out = String::new();

        let categories = self.canonical_category_count();
        self.render_title(&p, &mut out, categories);
        self.render_stats(&p, &mut out);

        let total = self.total_diffs();
        if total == 0 {
            return out;
        }

        // Summary counts.
        let _ = writeln!(
            out,
            "  {}: {}",
            p.bold("Differences"),
            p.fail(&format!("{} total", format_count(total as u64)))
        );
        let _ = writeln!(
            out,
            "    nodes: {}   attrs: {}   text-inline: {}   text-large: {}",
            format_count(self.node_deltas.len() as u64),
            format_count(self.attr_deltas.len() as u64),
            format_count(self.text_short_deltas.len() as u64),
            format_count(self.text_large_deltas.len() as u64)
        );
        out.push('\n');

        // Detail sections.
        self.render_node_section(&p, &mut out);
        self.render_attr_section(&p, &mut out);
        self.render_text_short_section(&p, &mut out);
        self.render_text_large_section(&p, &mut out);

        out
    }

    fn render_node_section(&self, p: &Paint, out: &mut String) {
        let deltas = self.node_deltas;
        if deltas.is_empty() {
            return;
        }
        section_header(p, out, "Node Subtrees", deltas.len());

        let max = self.limit.unwrap_or(deltas.len());
        for (i, d) in deltas.iter().take(max).enumerate() {
            let preview = self
                .left
                .nodes
                .get(&d.key)
                .or_else(|| self.right.nodes.get(&d.key))
                .map(|e| e.preview.descriptor.as_str())
                .unwrap_or("");

            let _ = writeln!(out, "   {}. {}", i + 1, p.bold(&d.key.path));
            let _ = writeln!(
                out,
                "      node  {}",
                format_delta_compact(p, d.left, d.right)
            );
            if !preview.is_empty() {
                let _ = writeln!(out, "      {}", p.dim(preview));
            }
        }
        section_overflow(out, deltas.len(), max);
        out.push('\n');
    }

    fn render_attr_section(&self, p: &Paint, out: &mut String) {
        let deltas = self.attr_deltas;
        if deltas.is_empty() {
            return;
        }
        section_header(p, out, "Attributes", deltas.len());

        let max = self.limit.unwrap_or(deltas.len());
        for (i, d) in deltas.iter().take(max).enumerate() {
            let _ = writeln!(out, "   {}. {}", i + 1, p.bold(&d.key.path));
            let _ = writeln!(
                out,
                "      attr  {}=\"{}\"",
                p.cyan(&d.key.name),
                clip(&d.key.value, PREVIEW_LINE_LIMIT)
            );
            let _ = writeln!(out, "      {}", format_delta_compact(p, d.left, d.right));
        }
        section_overflow(out, deltas.len(), max);
        out.push('\n');
    }

    fn render_text_short_section(&self, p: &Paint, out: &mut String) {
        let deltas = self.text_short_deltas;
        if deltas.is_empty() {
            return;
        }
        section_header(p, out, "Text Inline", deltas.len());

        let max = self.limit.unwrap_or(deltas.len());
        for (i, d) in deltas.iter().take(max).enumerate() {
            let _ = writeln!(out, "   {}. {}", i + 1, p.bold(&d.key.path));
            let _ = writeln!(
                out,
                "      text  \"{}\"",
                clip(&d.key.value, PREVIEW_LINE_LIMIT)
            );
            let _ = writeln!(out, "      {}", format_delta_compact(p, d.left, d.right));
        }
        section_overflow(out, deltas.len(), max);
        out.push('\n');
    }

    fn render_text_large_section(&self, p: &Paint, out: &mut String) {
        let deltas = self.text_large_deltas;
        if deltas.is_empty() {
            return;
        }
        section_header(p, out, "Text Large", deltas.len());

        let max = self.limit.unwrap_or(deltas.len());
        for (i, d) in deltas.iter().take(max).enumerate() {
            let _ = writeln!(out, "   {}. {}", i + 1, p.bold(&d.key.path));
            let _ = writeln!(
                out,
                "      text  {} bytes  blake3:{}",
                format_count(d.key.len),
                hex_prefix(&d.key.digest, 8)
            );
            let _ = writeln!(out, "      {}", format_delta_compact(p, d.left, d.right));
        }
        section_overflow(out, deltas.len(), max);
        out.push('\n');
    }

    // ── Semantic mode ───────────────────────────────────────────────────

    /// Classify all deltas into SemKind buckets. Each entry is a pre-formatted
    /// `SemanticEntry` holding the path, detail line, and optional preview.
    fn classify_all(&self) -> BTreeMap<SemKind, Vec<SemanticEntry>> {
        let mut by_kind: BTreeMap<SemKind, Vec<SemanticEntry>> = BTreeMap::new();
        for &kind in SemKind::ALL {
            by_kind.insert(kind, Vec::new());
        }

        for d in self.node_deltas {
            let kind = classify_node(d);
            let preview = self
                .left
                .nodes
                .get(&d.key)
                .or_else(|| self.right.nodes.get(&d.key))
                .map(|e| e.preview.descriptor.clone())
                .unwrap_or_default();
            by_kind.get_mut(&kind).unwrap().push(SemanticEntry {
                path: Arc::clone(&d.key.path),
                detail: format!("node  hash:{}", hex_prefix(&d.key.hash, 8)),
                delta_left: d.left,
                delta_right: d.right,
                preview,
            });
        }

        for d in self.attr_deltas {
            let kind = classify_attr(d);
            by_kind.get_mut(&kind).unwrap().push(SemanticEntry {
                path: Arc::clone(&d.key.path),
                detail: format!(
                    "attr  {}=\"{}\"",
                    d.key.name,
                    clip(&d.key.value, PREVIEW_LINE_LIMIT)
                ),
                delta_left: d.left,
                delta_right: d.right,
                preview: String::new(),
            });
        }

        for d in self.text_short_deltas {
            let kind = classify_text(&d.key.path);
            by_kind.get_mut(&kind).unwrap().push(SemanticEntry {
                path: Arc::clone(&d.key.path),
                detail: format!("text  \"{}\"", clip(&d.key.value, PREVIEW_LINE_LIMIT)),
                delta_left: d.left,
                delta_right: d.right,
                preview: String::new(),
            });
        }

        for d in self.text_large_deltas {
            let kind = classify_text(&d.key.path);
            by_kind.get_mut(&kind).unwrap().push(SemanticEntry {
                path: Arc::clone(&d.key.path),
                detail: format!(
                    "text  {} bytes  blake3:{}",
                    format_count(d.key.len),
                    hex_prefix(&d.key.digest, 8)
                ),
                delta_left: d.left,
                delta_right: d.right,
                preview: String::new(),
            });
        }

        by_kind
    }

    fn render_semantic(&self) -> String {
        let p = Paint::new(self.color);
        let mut out = String::new();

        // Classify once, use for both title and detail sections.
        let by_kind = self.classify_all();
        let categories = by_kind.values().filter(|v| !v.is_empty()).count();

        self.render_title(&p, &mut out, categories);
        self.render_stats(&p, &mut out);

        let total = self.total_diffs();
        if total == 0 {
            return out;
        }

        // ── Summary table ───────────────────────────────────────────────
        let non_zero: Vec<(&SemKind, &Vec<SemanticEntry>)> =
            by_kind.iter().filter(|(_, v)| !v.is_empty()).collect();
        let zero_count = SemKind::ALL.len() - non_zero.len();

        let table_rows: Vec<Vec<String>> = non_zero
            .iter()
            .map(|(kind, entries)| {
                vec![
                    kind.severity().trim().to_string(),
                    kind.label().to_string(),
                    format_count(entries.len() as u64),
                    kind.description().to_string(),
                ]
            })
            .collect();

        let _ = writeln!(out, "  {}", p.bold("Differences"));
        out.push_str(&render_table(
            &["Severity", "Category", ">Count", "Description"],
            &table_rows,
        ));

        if zero_count > 0 {
            let clean_names: Vec<&str> = by_kind
                .iter()
                .filter(|(_, v)| v.is_empty())
                .map(|(k, _)| k.label())
                .collect();
            let preview = if clean_names.len() <= 3 {
                clean_names.join(", ")
            } else {
                format!("{}, ...", clean_names[..3].join(", "))
            };
            let _ = writeln!(
                out,
                "    {}",
                p.dim(&format!("{zero_count} categories clean ({preview})"))
            );
        }

        let _ = writeln!(out, "    Total: {}\n", p.fail(&format_count(total as u64)));

        // ── Detail sections (only non-empty, severity order) ────────────
        for &kind in SemKind::ALL {
            let entries = &by_kind[&kind];
            if entries.is_empty() {
                continue;
            }

            let sev = kind.severity().trim();
            let header_text = format!(
                "{}: {} ({})",
                sev,
                kind.label(),
                format_count(entries.len() as u64)
            );
            section_header_semantic(&p, &mut out, sev, &header_text);

            let max = self.limit.unwrap_or(entries.len());
            for (i, entry) in entries.iter().take(max).enumerate() {
                let _ = writeln!(out, "   {}. {}", i + 1, p.bold(&entry.path));
                let _ = writeln!(out, "      {}", entry.detail);
                let _ = writeln!(
                    out,
                    "      {}",
                    format_delta_compact(&p, entry.delta_left, entry.delta_right)
                );
                if !entry.preview.is_empty() {
                    let _ = writeln!(out, "      {}", p.dim(&entry.preview));
                }
            }

            if entries.len() > max {
                let _ = writeln!(
                    out,
                    "   ... {} more (use --top={} or --report=file.txt for all)",
                    entries.len() - max,
                    entries.len()
                );
            }
            out.push('\n');
        }

        out
    }
}

// ── Types ───────────────────────────────────────────────────────────────────

struct SemanticEntry {
    path: Arc<str>,
    detail: String,
    delta_left: u64,
    delta_right: u64,
    preview: String,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Compact delta format: `left: 1  right: 0  delta: +1`
fn format_delta_compact(p: &Paint, left: u64, right: u64) -> String {
    let delta_signed = left as i128 - right as i128;
    let delta_str = if delta_signed >= 0 {
        p.yellow(&format!("+{delta_signed}"))
    } else {
        p.red(&delta_signed.to_string())
    };
    format!(
        "left: {}  right: {}  delta: {}",
        format_count(left),
        format_count(right),
        delta_str
    )
}

/// Section header with box-drawing line: `  ── Title (N) ──────────`
fn section_header(p: &Paint, out: &mut String, title: &str, count: usize) {
    let text = format!("── {} ({}) ", title, format_count(count as u64));
    let pad_len = 60usize.saturating_sub(text.len());
    let pad: String = "─".repeat(pad_len);
    let _ = writeln!(out, "  {}", p.blue(&format!("{text}{pad}")));
    out.push('\n');
}

/// Section header for semantic mode — colored by severity.
fn section_header_semantic(p: &Paint, out: &mut String, severity: &str, text: &str) {
    let full = format!("── {text} ");
    let pad_len = 60usize.saturating_sub(full.len());
    let pad: String = "─".repeat(pad_len);
    let _ = writeln!(out, "  {}", p.severity(severity, &format!("{full}{pad}")));
    out.push('\n');
}

/// Overflow message with hint.
fn section_overflow(out: &mut String, count: usize, max: usize) {
    if count > max {
        let _ = writeln!(
            out,
            "   ... {} more (use --top={} or --report=file.txt for all)",
            count - max,
            count
        );
    }
}
