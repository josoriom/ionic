use std::fmt::Write as _;
use std::time::Duration;

// ── Number / size formatting ────────────────────────────────────────────────

/// Format an integer with thousands separators: `1234567` → `"1,234,567"`.
pub fn format_count(n: u64) -> String {
    let raw = n.to_string();
    let len = raw.len();
    if len <= 3 {
        return raw;
    }
    let mut out = String::with_capacity(len + len / 3);
    for (i, ch) in raw.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Format a byte count in human-readable form: `354641815` → `"338.3 MB"`.
pub fn format_bytes(n: u64) -> String {
    const KB: f64 = 1_024.0;
    const MB: f64 = 1_048_576.0;
    const GB: f64 = 1_073_741_824.0;

    let f = n as f64;
    if f < KB {
        format!("{n} B")
    } else if f < MB {
        format!("{:.1} KB", f / KB)
    } else if f < GB {
        format!("{:.1} MB", f / MB)
    } else {
        format!("{:.1} GB", f / GB)
    }
}

// ── String helpers ──────────────────────────────────────────────────────────

/// Clip a string to `max` bytes on a char boundary, appending `"..."` if truncated.
pub fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut out = String::with_capacity(max + 3);
    let mut taken = 0usize;
    for ch in s.chars() {
        let n = ch.len_utf8();
        if taken + n > max {
            break;
        }
        out.push(ch);
        taken += n;
    }
    out.push_str("...");
    out
}

/// Encode the first `prefix_len` hex characters of a 32-byte hash.
/// Only encodes the bytes actually needed, not all 32 then truncating.
pub fn hex_prefix(bytes: &[u8; 32], prefix_len: usize) -> String {
    // Each byte produces 2 hex chars, so we need ceil(prefix_len/2) bytes.
    let bytes_needed = prefix_len.div_ceil(2).min(32);

    let mut out = String::with_capacity(prefix_len);
    for &b in &bytes[..bytes_needed] {
        out.push(nibble_to_hex(b >> 4));
        out.push(nibble_to_hex(b & 0x0f));
    }
    out.truncate(prefix_len);
    out
}

const fn nibble_to_hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'a' + (n - 10)) as char,
    }
}

pub fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1_000 {
        return format!("{ms}ms");
    }
    let s = d.as_secs_f64();
    format!("{s:.3}s")
}

/// Minimal ANSI coloring.
pub struct Paint {
    enabled: bool,
}

impl Paint {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    fn style(&self, code: &str, s: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_owned()
        }
    }

    pub fn red(&self, s: &str) -> String {
        self.style("1;31", s)
    }

    pub fn yellow(&self, s: &str) -> String {
        self.style("1;33", s)
    }

    pub fn blue(&self, s: &str) -> String {
        self.style("1;34", s)
    }

    pub fn bold(&self, s: &str) -> String {
        self.style("1", s)
    }

    pub fn dim(&self, s: &str) -> String {
        self.style("2", s)
    }

    pub fn cyan(&self, s: &str) -> String {
        self.style("1;36", s)
    }

    /// Bold green "PASS" label.
    pub fn pass(&self, s: &str) -> String {
        self.style("1;32", s)
    }

    /// Bold red "FAIL" label.
    pub fn fail(&self, s: &str) -> String {
        self.style("1;31", s)
    }

    /// Color by severity keyword.
    pub fn severity(&self, sev: &str, text: &str) -> String {
        match sev.trim() {
            "CRITICAL" => self.style("1;31", text),
            "HIGH" => self.style("1;33", text),
            "MEDIUM" => self.style("33", text),
            "LOW" => self.style("36", text),
            "INFO" => self.style("2", text),
            _ => text.to_owned(),
        }
    }
}

// ── Table rendering ─────────────────────────────────────────────────────────

/// Render a box-drawing table from column widths, headers, and rows.
///
/// Each row is a `Vec<String>` whose length must match `headers`.
/// Columns are left-aligned except those whose header starts with `>` (right-aligned,
/// the `>` is stripped from the displayed header).
pub fn render_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let ncols = headers.len();

    // Strip alignment prefix from headers.
    let clean_headers: Vec<&str> = headers
        .iter()
        .map(|h| h.strip_prefix('>').unwrap_or(h))
        .collect();
    let right_align: Vec<bool> = headers.iter().map(|h| h.starts_with('>')).collect();

    // Compute column widths.
    let mut widths: Vec<usize> = clean_headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate().take(ncols) {
            widths[i] = widths[i].max(cell.len());
        }
    }

    let mut out = String::new();

    // Top border.
    write_border(&mut out, &widths, '┌', '┬', '┐');
    // Header row.
    write_row(
        &mut out,
        &clean_headers
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        &widths,
        &right_align,
    );
    // Separator.
    write_border(&mut out, &widths, '├', '┼', '┤');
    // Data rows.
    for row in rows {
        write_row(&mut out, row, &widths, &right_align);
    }
    // Bottom border.
    write_border(&mut out, &widths, '└', '┴', '┘');

    out
}

fn write_border(out: &mut String, widths: &[usize], left: char, mid: char, right: char) {
    out.push_str("  ");
    out.push(left);
    for (i, &w) in widths.iter().enumerate() {
        for _ in 0..w + 2 {
            out.push('─');
        }
        out.push(if i + 1 < widths.len() { mid } else { right });
    }
    out.push('\n');
}

fn write_row(out: &mut String, cells: &[String], widths: &[usize], right_align: &[bool]) {
    out.push_str("  │");
    for (i, cell) in cells.iter().enumerate() {
        let w = widths[i];
        if right_align[i] {
            let _ = write!(out, " {cell:>w$} │");
        } else {
            let _ = write!(out, " {cell:<w$} │");
        }
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── format_count ──────────────────────────────────────────────────────

    #[test]
    fn count_zero() {
        assert_eq!(format_count(0), "0");
    }

    #[test]
    fn count_small() {
        assert_eq!(format_count(42), "42");
    }

    #[test]
    fn count_thousands() {
        assert_eq!(format_count(1_234), "1,234");
    }

    #[test]
    fn count_millions() {
        assert_eq!(format_count(1_234_567), "1,234,567");
    }

    #[test]
    fn count_exact_boundary() {
        assert_eq!(format_count(1_000), "1,000");
    }

    // ── format_bytes ─────────────────────────────────────────────────────

    #[test]
    fn bytes_small() {
        assert_eq!(format_bytes(512), "512 B");
    }

    #[test]
    fn bytes_kb() {
        assert_eq!(format_bytes(10_240), "10.0 KB");
    }

    #[test]
    fn bytes_mb() {
        assert_eq!(format_bytes(354_641_815), "338.2 MB");
    }

    #[test]
    fn bytes_gb() {
        assert_eq!(format_bytes(2_147_483_648), "2.0 GB");
    }

    // ── clip ──────────────────────────────────────────────────────────────

    #[test]
    fn clip_short_string_unchanged() {
        assert_eq!(clip("hello", 10), "hello");
    }

    #[test]
    fn clip_exact_length() {
        assert_eq!(clip("hello", 5), "hello");
    }

    #[test]
    fn clip_truncates_with_ellipsis() {
        assert_eq!(clip("hello world", 5), "hello...");
    }

    #[test]
    fn clip_respects_char_boundary() {
        // 'e' with acute is 2 bytes in UTF-8.
        let s = "\u{00e9}ab";
        // max=1 can't fit the 2-byte char, so it clips before it.
        assert_eq!(clip(s, 1), "...");
    }

    #[test]
    fn clip_empty_string() {
        assert_eq!(clip("", 10), "");
    }

    // ── hex_prefix ───────────────────────────────────────────────────────

    #[test]
    fn hex_prefix_full() {
        let bytes = [0xab; 32];
        assert_eq!(hex_prefix(&bytes, 64), "ab".repeat(32));
    }

    #[test]
    fn hex_prefix_short() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xde;
        bytes[1] = 0xad;
        assert_eq!(hex_prefix(&bytes, 4), "dead");
    }

    #[test]
    fn hex_prefix_odd_length() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xab;
        // prefix_len=3 -> "ab?" where ? is first nibble of bytes[1]
        assert_eq!(hex_prefix(&bytes, 3), "ab0");
    }

    #[test]
    fn hex_prefix_zero() {
        let bytes = [0u8; 32];
        assert_eq!(hex_prefix(&bytes, 0), "");
    }

    // ── format_duration ──────────────────────────────────────────────────

    #[test]
    fn duration_millis() {
        assert_eq!(format_duration(Duration::from_millis(42)), "42ms");
    }

    #[test]
    fn duration_seconds() {
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.500s");
    }

    // ── Paint ────────────────────────────────────────────────────────────

    #[test]
    fn paint_disabled_no_ansi() {
        let p = Paint::new(false);
        assert_eq!(p.red("error"), "error");
    }

    #[test]
    fn paint_enabled_wraps_ansi() {
        let p = Paint::new(true);
        assert_eq!(p.red("x"), "\x1b[1;31mx\x1b[0m");
    }

    #[test]
    fn paint_dim() {
        let p = Paint::new(true);
        assert_eq!(p.dim("x"), "\x1b[2mx\x1b[0m");
    }

    #[test]
    fn paint_pass_fail() {
        let p = Paint::new(true);
        assert!(p.pass("PASS").contains("32m"));
        assert!(p.fail("FAIL").contains("31m"));
    }

    #[test]
    fn paint_severity_colors() {
        let p = Paint::new(true);
        assert!(p.severity("CRITICAL", "x").contains("31m"));
        assert!(p.severity("HIGH", "x").contains("33m"));
        assert!(p.severity("INFO", "x").contains("2m"));
    }

    // ── render_table ─────────────────────────────────────────────────────

    #[test]
    fn table_basic() {
        let out = render_table(
            &["Name", ">Count"],
            &[
                vec!["alpha".into(), "10".into()],
                vec!["beta".into(), "200".into()],
            ],
        );
        assert!(out.contains("alpha"));
        assert!(out.contains("200"));
        // Box-drawing chars present.
        assert!(out.contains('┌'));
        assert!(out.contains('┘'));
    }

    #[test]
    fn table_empty_rows() {
        let out = render_table(&["A", "B"], &[]);
        // Should still have header + borders.
        assert!(out.contains('┌'));
        assert!(out.contains('┘'));
    }
}
