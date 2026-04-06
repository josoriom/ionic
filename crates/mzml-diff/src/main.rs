use std::{
    fs::File,
    hash::Hash,
    io::{BufReader, IsTerminal, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use blake3::Hasher;
use clap::Parser;
use hashbrown::HashMap;
use quick_xml::{
    Reader,
    encoding::Decoder,
    events::{BytesStart, Event},
};
use rayon::join;

const INPUT_BUFFER_CAPACITY: usize = 1024 * 1024;
const XML_EVENT_BUFFER_CAPACITY: usize = 16 * 1024;
const DEFAULT_TOP: usize = 80;
const PREVIEW_ATTR_LIMIT: usize = 4;
const PREVIEW_VALUE_LIMIT: usize = 64;
const PREVIEW_LINE_LIMIT: usize = 180;

#[derive(Debug, Parser)]
#[command(
    name = "mzml-diff",
    version,
    about = "Canonical mzML diff (order-insensitive, content-aware)"
)]
struct Args {
    #[arg(long, short = 'l')]
    left: PathBuf,

    #[arg(long, short = 'r')]
    right: PathBuf,

    #[arg(long, short = 'o')]
    report: Option<PathBuf>,

    #[arg(long, default_value_t = DEFAULT_TOP)]
    top: usize,

    #[arg(long, default_value_t = 256)]
    inline_text_max: usize,

    #[arg(long, action = clap::ArgAction::SetTrue, default_value_t = false)]
    no_color: bool,
}

#[derive(Debug, Default, Clone)]
struct Totals {
    nodes: u64,
    attrs: u64,
    text_nodes: u64,
    text_bytes: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct NodeKey {
    path: String,
    hash: [u8; 32],
}

#[derive(Debug, Clone)]
struct NodePreview {
    descriptor: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct AttrKey {
    path: String,
    name: String,
    value: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct TextShortKey {
    path: String,
    value: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct TextLargeKey {
    path: String,
    len: u64,
    digest: [u8; 32],
}

#[derive(Default)]
struct CanonIndex {
    totals: Totals,
    node_counts: HashMap<NodeKey, u64>,
    node_preview: HashMap<NodeKey, NodePreview>,
    attr_counts: HashMap<AttrKey, u64>,
    text_short_counts: HashMap<TextShortKey, u64>,
    text_large_counts: HashMap<TextLargeKey, u64>,
}

struct NodeState {
    name: String,
    attrs: Vec<(String, String)>,

    text_hasher: Hasher,
    text_len: u64,
    text_preview: String,
    text_preview_complete: bool,
    has_non_ws_text: bool,

    child_counts: HashMap<[u8; 32], u32>,
    child_total: u64,
}

struct FinalNode {
    hash: [u8; 32],
    preview: NodePreview,
    text_short: Option<String>,
    text_large: Option<(u64, [u8; 32])>,
}

#[derive(Debug, Clone)]
struct Delta<K> {
    key: K,
    left: u64,
    right: u64,
}

struct Paint {
    enabled: bool,
}

impl Paint {
    fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    fn style(&self, code: &str, s: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    fn red(&self, s: &str) -> String {
        self.style("1;31", s)
    }

    fn green(&self, s: &str) -> String {
        self.style("1;32", s)
    }

    fn yellow(&self, s: &str) -> String {
        self.style("1;33", s)
    }

    fn blue(&self, s: &str) -> String {
        self.style("1;34", s)
    }

    fn bold(&self, s: &str) -> String {
        self.style("1", s)
    }
}

impl NodeState {
    fn new(name: String, attrs: Vec<(String, String)>) -> Self {
        Self {
            name,
            attrs,
            text_hasher: Hasher::new(),
            text_len: 0,
            text_preview: String::new(),
            text_preview_complete: true,
            has_non_ws_text: false,
            child_counts: HashMap::new(),
            child_total: 0,
        }
    }

    fn absorb_text(&mut self, s: &str, inline_text_max: usize) {
        self.has_non_ws_text = true;
        self.text_len = self.text_len.saturating_add(s.len() as u64);
        self.text_hasher.update(s.as_bytes());

        if self.text_preview_complete {
            let next_len = self.text_preview.len().saturating_add(s.len());
            if next_len <= inline_text_max {
                self.text_preview.push_str(s);
            } else {
                self.text_preview_complete = false;
            }
        }
    }

    fn absorb_child_hash(&mut self, hash: [u8; 32]) {
        *self.child_counts.entry(hash).or_insert(0) += 1;
        self.child_total += 1;
    }

    fn finalize(self, inline_text_max: usize) -> FinalNode {
        let mut sorted_children: Vec<([u8; 32], u32)> = self.child_counts.into_iter().collect();
        sorted_children.sort_unstable_by(|a, b| a.0.cmp(&b.0));

        let text_digest = if self.has_non_ws_text {
            Some(*self.text_hasher.finalize().as_bytes())
        } else {
            None
        };

        let mut h = Hasher::new();
        h.update(b"NODE\0");
        h.update(self.name.as_bytes());
        h.update(b"\0");

        h.update(&(self.attrs.len() as u64).to_le_bytes());
        for (k, v) in &self.attrs {
            h.update(k.as_bytes());
            h.update(b"\0");
            h.update(v.as_bytes());
            h.update(b"\0");
        }

        match text_digest {
            Some(d) => {
                h.update(&[1]);
                h.update(&self.text_len.to_le_bytes());
                h.update(&d);
            }
            None => {
                h.update(&[0]);
            }
        }

        h.update(&self.child_total.to_le_bytes());
        h.update(&(sorted_children.len() as u64).to_le_bytes());
        for (child_hash, cnt) in &sorted_children {
            h.update(child_hash);
            h.update(&cnt.to_le_bytes());
        }

        let hash = *h.finalize().as_bytes();

        let text_short = if self.has_non_ws_text
            && self.text_preview_complete
            && (self.text_len as usize) <= inline_text_max
        {
            Some(self.text_preview.clone())
        } else {
            None
        };

        let text_large = match (self.has_non_ws_text, text_short.as_ref(), text_digest) {
            (true, None, Some(d)) => Some((self.text_len, d)),
            _ => None,
        };

        let descriptor = render_node_descriptor(
            &self.name,
            &self.attrs,
            self.child_total,
            sorted_children.len(),
            self.text_len,
            text_digest,
        );

        FinalNode {
            hash,
            preview: NodePreview { descriptor },
            text_short,
            text_large,
        }
    }
}

fn main() -> Result<(), String> {
    let args = Args::parse();

    let all_start = Instant::now();
    let ((left_res, left_elapsed), (right_res, right_elapsed)) = join(
        || timed_build_index(&args.left, args.inline_text_max),
        || timed_build_index(&args.right, args.inline_text_max),
    );

    let left_index = left_res?;
    let right_index = right_res?;
    let total_elapsed = all_start.elapsed();

    let mut node_deltas = diff_counts(&left_index.node_counts, &right_index.node_counts);
    let mut attr_deltas = diff_counts(&left_index.attr_counts, &right_index.attr_counts);
    let mut text_short_deltas = diff_counts(
        &left_index.text_short_counts,
        &right_index.text_short_counts,
    );
    let mut text_large_deltas = diff_counts(
        &left_index.text_large_counts,
        &right_index.text_large_counts,
    );

    sort_deltas(&mut node_deltas, |k| &k.path, |_| "");
    sort_deltas(&mut attr_deltas, |k| &k.path, |k| &k.name);
    sort_deltas(&mut text_short_deltas, |k| &k.path, |_| "");
    sort_deltas(&mut text_large_deltas, |k| &k.path, |_| "");

    let color_enabled = !args.no_color && std::io::stdout().is_terminal();
    let terminal_report = render_report(
        &args,
        &left_index,
        &right_index,
        &node_deltas,
        &attr_deltas,
        &text_short_deltas,
        &text_large_deltas,
        left_elapsed,
        right_elapsed,
        total_elapsed,
        Some(args.top),
        color_enabled,
    );

    println!("{terminal_report}");

    if let Some(path) = &args.report {
        let plain_report = render_report(
            &args,
            &left_index,
            &right_index,
            &node_deltas,
            &attr_deltas,
            &text_short_deltas,
            &text_large_deltas,
            left_elapsed,
            right_elapsed,
            total_elapsed,
            None,
            false,
        );
        let mut f = File::create(path)
            .map_err(|e| format!("cannot create report file {}: {e}", path.display()))?;
        f.write_all(plain_report.as_bytes())
            .map_err(|e| format!("cannot write report file {}: {e}", path.display()))?;
    }

    let has_diffs = !node_deltas.is_empty()
        || !attr_deltas.is_empty()
        || !text_short_deltas.is_empty()
        || !text_large_deltas.is_empty();

    if has_diffs {
        return Err("canonical differences detected".to_string());
    }

    Ok(())
}

fn timed_build_index(
    path: &Path,
    inline_text_max: usize,
) -> (Result<CanonIndex, String>, std::time::Duration) {
    let start = Instant::now();
    let out = build_index(path, inline_text_max);
    (out, start.elapsed())
}

fn build_index(path: &Path, inline_text_max: usize) -> Result<CanonIndex, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut reader = Reader::from_reader(BufReader::with_capacity(INPUT_BUFFER_CAPACITY, file));
    reader.config_mut().trim_text(false);

    let mut index = CanonIndex::default();
    let mut names: Vec<String> = Vec::with_capacity(64);
    let mut states: Vec<NodeState> = Vec::with_capacity(64);
    let mut buf = Vec::with_capacity(XML_EVENT_BUFFER_CAPACITY);

    loop {
        match reader.read_event_into(&mut buf).map_err(|e| {
            format!(
                "xml read error in {} at byte {}: {e}",
                path.display(),
                reader.buffer_position()
            )
        })? {
            Event::Start(e) => {
                let name = decode_name(e.name().as_ref());
                let attrs = parse_attrs(&e, reader.decoder())?;
                let path_now = compose_path(&names, Some(&name));

                index.totals.nodes += 1;
                index.totals.attrs = index.totals.attrs.saturating_add(attrs.len() as u64);
                record_attrs(&mut index, &path_now, &attrs);

                names.push(name.clone());
                states.push(NodeState::new(name, attrs));
            }
            Event::Empty(e) => {
                let name = decode_name(e.name().as_ref());
                let attrs = parse_attrs(&e, reader.decoder())?;
                let path_now = compose_path(&names, Some(&name));

                index.totals.nodes += 1;
                index.totals.attrs = index.totals.attrs.saturating_add(attrs.len() as u64);
                record_attrs(&mut index, &path_now, &attrs);

                let node = NodeState::new(name, attrs).finalize(inline_text_max);
                record_final_node(&mut index, &path_now, &node);

                if let Some(parent) = states.last_mut() {
                    parent.absorb_child_hash(node.hash);
                }
            }
            Event::Text(t) => {
                if let Some(curr) = states.last_mut() {
                    let txt = t
                        .decode()
                        .map_err(|e| format!("text decode error in {}: {e}", path.display()))?;
                    if !txt.trim().is_empty() {
                        curr.absorb_text(txt.as_ref(), inline_text_max);
                    }
                }
            }
            Event::CData(c) => {
                if let Some(curr) = states.last_mut() {
                    let txt = c
                        .decode()
                        .map_err(|e| format!("cdata decode error in {}: {e}", path.display()))?;
                    if !txt.trim().is_empty() {
                        curr.absorb_text(txt.as_ref(), inline_text_max);
                    }
                }
            }
            Event::End(e) => {
                let close_name = decode_name(e.name().as_ref());
                let node_state = states.pop().ok_or_else(|| {
                    format!(
                        "unexpected closing tag </{close_name}> in {}",
                        path.display()
                    )
                })?;

                let open_name = names.pop().ok_or_else(|| {
                    format!(
                        "stack underflow for closing tag </{close_name}> in {}",
                        path.display()
                    )
                })?;

                if open_name != close_name {
                    return Err(format!(
                        "mismatched closing tag in {}: opened <{}> closed </{}>",
                        path.display(),
                        open_name,
                        close_name
                    ));
                }

                let path_now = compose_path(&names, Some(&open_name));
                let node = node_state.finalize(inline_text_max);
                record_final_node(&mut index, &path_now, &node);

                if let Some(parent) = states.last_mut() {
                    parent.absorb_child_hash(node.hash);
                }
            }
            Event::Eof => break,
            _ => {}
        }

        buf.clear();
    }

    if !states.is_empty() || !names.is_empty() {
        return Err(format!(
            "unexpected EOF in {} (unclosed tags remaining)",
            path.display()
        ));
    }

    Ok(index)
}

fn parse_attrs(start: &BytesStart, decoder: Decoder) -> Result<Vec<(String, String)>, String> {
    let mut attrs = Vec::new();

    for attr in start.attributes().with_checks(false) {
        let attr = attr.map_err(|e| format!("attribute parse failed: {e}"))?;
        let key = decode_name(attr.key.as_ref());
        let value = attr
            .decode_and_unescape_value(decoder)
            .map_err(|e| format!("attribute value decode failed: {e}"))?
            .into_owned();
        attrs.push((key, value));
    }

    attrs.sort_unstable();
    Ok(attrs)
}

fn decode_name(raw: &[u8]) -> String {
    let mut name = raw;
    if name.first() == Some(&b'{') {
        if let Some(end) = name.iter().position(|&b| b == b'}') {
            name = &name[end + 1..];
        }
    }
    if let Some(colon) = name.iter().rposition(|&b| b == b':') {
        name = &name[colon + 1..];
    }
    String::from_utf8_lossy(name).into_owned()
}

fn compose_path(stack: &[String], current: Option<&str>) -> String {
    let mut len = 1usize;
    for s in stack {
        len = len.saturating_add(s.len() + 1);
    }
    if let Some(s) = current {
        len = len.saturating_add(s.len() + 1);
    }

    let mut out = String::with_capacity(len);
    for s in stack {
        out.push('/');
        out.push_str(s);
    }
    if let Some(s) = current {
        out.push('/');
        out.push_str(s);
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

fn record_attrs(index: &mut CanonIndex, path: &str, attrs: &[(String, String)]) {
    for (k, v) in attrs {
        let key = AttrKey {
            path: path.to_string(),
            name: k.clone(),
            value: v.clone(),
        };
        *index.attr_counts.entry(key).or_insert(0) += 1;
    }
}

fn record_final_node(index: &mut CanonIndex, path: &str, node: &FinalNode) {
    let key = NodeKey {
        path: path.to_string(),
        hash: node.hash,
    };

    *index.node_counts.entry(key.clone()).or_insert(0) += 1;
    index
        .node_preview
        .entry(key)
        .or_insert_with(|| node.preview.clone());

    if let Some(short_text) = &node.text_short {
        index.totals.text_nodes += 1;
        index.totals.text_bytes = index
            .totals
            .text_bytes
            .saturating_add(short_text.len() as u64);
        let tk = TextShortKey {
            path: path.to_string(),
            value: short_text.clone(),
        };
        *index.text_short_counts.entry(tk).or_insert(0) += 1;
    }

    if let Some((len, digest)) = node.text_large {
        index.totals.text_nodes += 1;
        index.totals.text_bytes = index.totals.text_bytes.saturating_add(len);
        let tk = TextLargeKey {
            path: path.to_string(),
            len,
            digest,
        };
        *index.text_large_counts.entry(tk).or_insert(0) += 1;
    }
}

fn render_node_descriptor(
    name: &str,
    attrs: &[(String, String)],
    child_total: u64,
    child_unique: usize,
    text_len: u64,
    text_digest: Option<[u8; 32]>,
) -> String {
    let mut out = String::new();
    out.push('<');
    out.push_str(name);

    for (k, v) in attrs.iter().take(PREVIEW_ATTR_LIMIT) {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&clip_value(v, PREVIEW_VALUE_LIMIT));
        out.push('"');
    }

    if attrs.len() > PREVIEW_ATTR_LIMIT {
        let extra = attrs.len() - PREVIEW_ATTR_LIMIT;
        out.push_str(" ...+");
        out.push_str(&extra.to_string());
        out.push_str(" attrs");
    }

    out.push('>');
    out.push_str(" children=");
    out.push_str(&child_total.to_string());
    out.push('/');
    out.push_str(&child_unique.to_string());

    if let Some(d) = text_digest {
        out.push_str(" text_len=");
        out.push_str(&text_len.to_string());
        out.push_str(" text_blake3=");
        out.push_str(&hex_prefix(&d, 16));
    }

    clip_line(&out, PREVIEW_LINE_LIMIT)
}

fn clip_value(v: &str, max: usize) -> String {
    if v.len() <= max {
        return v.to_string();
    }

    let mut out = String::with_capacity(max + 3);
    let mut taken = 0usize;
    for ch in v.chars() {
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

fn clip_line(v: &str, max: usize) -> String {
    if v.len() <= max {
        return v.to_string();
    }
    clip_value(v, max)
}

fn diff_counts<K>(left: &HashMap<K, u64>, right: &HashMap<K, u64>) -> Vec<Delta<K>>
where
    K: Eq + Hash + Clone,
{
    let mut out = Vec::new();

    for (k, lv) in left {
        let rv = right.get(k).copied().unwrap_or(0);
        if *lv != rv {
            out.push(Delta {
                key: k.clone(),
                left: *lv,
                right: rv,
            });
        }
    }

    for (k, rv) in right {
        if !left.contains_key(k) {
            out.push(Delta {
                key: k.clone(),
                left: 0,
                right: *rv,
            });
        }
    }

    out
}

fn sort_deltas<K, P, T>(deltas: &mut [Delta<K>], path_key: P, tie_key: T)
where
    P: Fn(&K) -> &str,
    T: Fn(&K) -> &str,
{
    deltas.sort_unstable_by(|a, b| {
        let a_delta = a.left.abs_diff(a.right);
        let b_delta = b.left.abs_diff(b.right);
        b_delta
            .cmp(&a_delta)
            .then_with(|| path_key(&a.key).cmp(path_key(&b.key)))
            .then_with(|| tie_key(&a.key).cmp(tie_key(&b.key)))
    });
}

#[allow(clippy::too_many_arguments)]
fn render_report(
    args: &Args,
    left: &CanonIndex,
    right: &CanonIndex,
    node_deltas: &[Delta<NodeKey>],
    attr_deltas: &[Delta<AttrKey>],
    text_short_deltas: &[Delta<TextShortKey>],
    text_large_deltas: &[Delta<TextLargeKey>],
    left_elapsed: std::time::Duration,
    right_elapsed: std::time::Duration,
    total_elapsed: std::time::Duration,
    limit: Option<usize>,
    color: bool,
) -> String {
    let p = Paint::new(color);
    let mut out = String::new();

    out.push_str(&format!(
        "{}\n",
        p.bold("mzML Canonical Diff (order-insensitive)")
    ));
    out.push_str(&format!(
        "left:  {}\nright: {}\n",
        args.left.display(),
        args.right.display()
    ));

    out.push_str(&format!(
        "parse_time: left={} right={} wall={} (parallel)\n",
        format_duration(left_elapsed),
        format_duration(right_elapsed),
        format_duration(total_elapsed)
    ));

    out.push_str("\n");
    out.push_str(&format!(
        "left_totals:  nodes={} attrs={} text_nodes={} text_bytes={}\n",
        left.totals.nodes, left.totals.attrs, left.totals.text_nodes, left.totals.text_bytes
    ));
    out.push_str(&format!(
        "right_totals: nodes={} attrs={} text_nodes={} text_bytes={}\n",
        right.totals.nodes, right.totals.attrs, right.totals.text_nodes, right.totals.text_bytes
    ));

    let total_diff =
        node_deltas.len() + attr_deltas.len() + text_short_deltas.len() + text_large_deltas.len();
    let total_diff_label = if total_diff == 0 {
        p.green("0")
    } else {
        p.red(&total_diff.to_string())
    };

    out.push_str("\n");
    out.push_str(&format!("difference_keys_total: {total_diff_label}\n"));
    out.push_str(&format!(
        "node_subtrees={} attr_values={} text_short={} text_large={}\n",
        node_deltas.len(),
        attr_deltas.len(),
        text_short_deltas.len(),
        text_large_deltas.len()
    ));

    if total_diff == 0 {
        out.push_str("\n");
        out.push_str(&format!(
            "{}\n",
            p.green("No canonical content differences detected.")
        ));
        return out;
    }

    out.push_str("\n");
    render_node_section(
        &mut out,
        &p,
        "Node Subtree Differences",
        node_deltas,
        left,
        right,
        limit,
    );
    out.push_str("\n");
    render_attr_section(&mut out, &p, "Attribute Differences", attr_deltas, limit);
    out.push_str("\n");
    render_text_short_section(
        &mut out,
        &p,
        "Text Differences (inline)",
        text_short_deltas,
        limit,
    );
    out.push_str("\n");
    render_text_large_section(
        &mut out,
        &p,
        "Text Differences (large payload)",
        text_large_deltas,
        limit,
    );

    out
}

fn render_node_section(
    out: &mut String,
    p: &Paint,
    title: &str,
    deltas: &[Delta<NodeKey>],
    left: &CanonIndex,
    right: &CanonIndex,
    limit: Option<usize>,
) {
    let count = deltas.len();
    out.push_str(&format!("{} ({})\n", p.blue(title), count));

    let max = limit.unwrap_or(count);
    for (i, d) in deltas.iter().take(max).enumerate() {
        let delta_signed = d.left as i128 - d.right as i128;
        let sign = if delta_signed >= 0 {
            p.yellow(&format!("+{delta_signed}"))
        } else {
            p.red(&delta_signed.to_string())
        };

        let preview = left
            .node_preview
            .get(&d.key)
            .or_else(|| right.node_preview.get(&d.key))
            .map(|v| v.descriptor.as_str())
            .unwrap_or("<preview unavailable>");

        out.push_str(&format!(
            "{}. delta={} left={} right={} path={} hash={} sample={}\n",
            i + 1,
            sign,
            d.left,
            d.right,
            d.key.path,
            hex_prefix(&d.key.hash, 16),
            preview
        ));
    }

    if let Some(max) = limit {
        if count > max {
            out.push_str(&format!(
                "... {} more entries omitted in terminal output\n",
                count - max
            ));
        }
    }
}

fn render_attr_section(
    out: &mut String,
    p: &Paint,
    title: &str,
    deltas: &[Delta<AttrKey>],
    limit: Option<usize>,
) {
    let count = deltas.len();
    out.push_str(&format!("{} ({})\n", p.blue(title), count));

    let max = limit.unwrap_or(count);
    for (i, d) in deltas.iter().take(max).enumerate() {
        let delta_signed = d.left as i128 - d.right as i128;
        let sign = if delta_signed >= 0 {
            p.yellow(&format!("+{delta_signed}"))
        } else {
            p.red(&delta_signed.to_string())
        };

        out.push_str(&format!(
            "{}. delta={} left={} right={} path={} attr={} value=\"{}\"\n",
            i + 1,
            sign,
            d.left,
            d.right,
            d.key.path,
            d.key.name,
            clip_line(&d.key.value, PREVIEW_LINE_LIMIT)
        ));
    }

    if let Some(max) = limit {
        if count > max {
            out.push_str(&format!(
                "... {} more entries omitted in terminal output\n",
                count - max
            ));
        }
    }
}

fn render_text_short_section(
    out: &mut String,
    p: &Paint,
    title: &str,
    deltas: &[Delta<TextShortKey>],
    limit: Option<usize>,
) {
    let count = deltas.len();
    out.push_str(&format!("{} ({})\n", p.blue(title), count));

    let max = limit.unwrap_or(count);
    for (i, d) in deltas.iter().take(max).enumerate() {
        let delta_signed = d.left as i128 - d.right as i128;
        let sign = if delta_signed >= 0 {
            p.yellow(&format!("+{delta_signed}"))
        } else {
            p.red(&delta_signed.to_string())
        };

        out.push_str(&format!(
            "{}. delta={} left={} right={} path={} text=\"{}\"\n",
            i + 1,
            sign,
            d.left,
            d.right,
            d.key.path,
            clip_line(&d.key.value, PREVIEW_LINE_LIMIT)
        ));
    }

    if let Some(max) = limit {
        if count > max {
            out.push_str(&format!(
                "... {} more entries omitted in terminal output\n",
                count - max
            ));
        }
    }
}

fn render_text_large_section(
    out: &mut String,
    p: &Paint,
    title: &str,
    deltas: &[Delta<TextLargeKey>],
    limit: Option<usize>,
) {
    let count = deltas.len();
    out.push_str(&format!("{} ({})\n", p.blue(title), count));

    let max = limit.unwrap_or(count);
    for (i, d) in deltas.iter().take(max).enumerate() {
        let delta_signed = d.left as i128 - d.right as i128;
        let sign = if delta_signed >= 0 {
            p.yellow(&format!("+{delta_signed}"))
        } else {
            p.red(&delta_signed.to_string())
        };

        out.push_str(&format!(
            "{}. delta={} left={} right={} path={} text_len={} text_blake3={}\n",
            i + 1,
            sign,
            d.left,
            d.right,
            d.key.path,
            d.key.len,
            hex_prefix(&d.key.digest, 16)
        ));
    }

    if let Some(max) = limit {
        if count > max {
            out.push_str(&format!(
                "... {} more entries omitted in terminal output\n",
                count - max
            ));
        }
    }
}

fn hex_prefix(bytes: &[u8; 32], prefix_len: usize) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(nibble_to_hex(b >> 4));
        out.push(nibble_to_hex(b & 0x0f));
    }
    if prefix_len >= out.len() {
        out
    } else {
        out[..prefix_len].to_string()
    }
}

fn nibble_to_hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => '0',
    }
}

fn format_duration(d: std::time::Duration) -> String {
    let ms = d.as_millis();
    if ms < 1_000 {
        return format!("{ms}ms");
    }
    let s = d.as_secs_f64();
    format!("{s:.3}s")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../parser/data/mzml")
            .join(name)
    }

    #[test]
    fn identical_fixture_has_no_differences() {
        let path = fixture("tiny1.mzML0.99.0.mzML");
        let left = build_index(&path, 256).expect("left parse");
        let right = build_index(&path, 256).expect("right parse");

        assert!(diff_counts(&left.node_counts, &right.node_counts).is_empty());
        assert!(diff_counts(&left.attr_counts, &right.attr_counts).is_empty());
        assert!(diff_counts(&left.text_short_counts, &right.text_short_counts).is_empty());
        assert!(diff_counts(&left.text_large_counts, &right.text_large_counts).is_empty());
    }

    #[test]
    fn different_fixture_detects_differences() {
        let left = build_index(&fixture("tiny1.mzML0.99.0.mzML"), 256).expect("left parse");
        let right = build_index(&fixture("tiny1.mzML0.99.1.mzML"), 256).expect("right parse");

        let any_diff = !diff_counts(&left.node_counts, &right.node_counts).is_empty()
            || !diff_counts(&left.attr_counts, &right.attr_counts).is_empty()
            || !diff_counts(&left.text_short_counts, &right.text_short_counts).is_empty()
            || !diff_counts(&left.text_large_counts, &right.text_large_counts).is_empty();

        assert!(any_diff);
    }
}
