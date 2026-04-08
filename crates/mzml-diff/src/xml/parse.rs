use std::{
    collections::HashSet,
    fs::File,
    io::BufReader,
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use blake3::Hasher;
use hashbrown::HashMap;
use quick_xml::{
    Reader,
    encoding::Decoder,
    events::{BytesStart, Event},
};

use super::normalize::normalize_cv_attrs;
use super::path::PathBuilder;
use crate::cli::DiffMode;
use crate::tables;

const INPUT_BUFFER_CAPACITY: usize = 1024 * 1024;
const XML_EVENT_BUFFER_CAPACITY: usize = 16 * 1024;

pub const PREVIEW_ATTR_LIMIT: usize = 4;
pub const PREVIEW_VALUE_LIMIT: usize = 64;
pub const PREVIEW_LINE_LIMIT: usize = 180;

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct Totals {
    pub nodes: u64,
    pub attrs: u64,
    pub text_nodes: u64,
    pub text_bytes: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct NodeKey {
    pub path: Arc<str>,
    pub hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct NodePreview {
    pub descriptor: String,
}

/// Combined count + preview for a node key. Eliminates duplicate `NodeKey`
/// storage that previously existed across separate `node_counts` and
/// `node_preview` maps.
#[derive(Debug, Clone)]
pub struct NodeEntry {
    pub count: u64,
    pub preview: NodePreview,
}

impl crate::diff::structural::Counted for NodeEntry {
    #[inline]
    fn count(&self) -> u64 {
        self.count
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct AttrKey {
    pub path: Arc<str>,
    pub name: Arc<str>,
    pub value: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct TextShortKey {
    pub path: Arc<str>,
    pub value: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct TextLargeKey {
    pub path: Arc<str>,
    pub len: u64,
    pub digest: [u8; 32],
}

pub struct CanonIndex {
    pub totals: Totals,
    pub nodes: HashMap<NodeKey, NodeEntry>,
    pub attr_counts: HashMap<AttrKey, u64>,
    pub text_short_counts: HashMap<TextShortKey, u64>,
    pub text_large_counts: HashMap<TextLargeKey, u64>,
}

impl Default for CanonIndex {
    fn default() -> Self {
        Self {
            totals: Totals::default(),
            nodes: HashMap::new(),
            attr_counts: HashMap::new(),
            text_short_counts: HashMap::new(),
            text_large_counts: HashMap::new(),
        }
    }
}

// ── String interner ──────────────────────────────────────────────────────────

/// A lightweight string interner that deduplicates via `Arc<str>`.
///
/// Used during `build_index` to ensure the ~50 unique XML paths and ~20-30
/// attribute names share a single heap allocation rather than being cloned
/// millions of times. Each file is parsed with its own interner (safe for
/// rayon parallelism since `Arc<str>` is `Send`).
struct Interner {
    set: HashSet<Arc<str>>,
}

impl Interner {
    fn new() -> Self {
        Self {
            set: HashSet::new(),
        }
    }

    /// Return a shared `Arc<str>` for `s`, reusing a previous allocation if
    /// one exists.
    fn intern(&mut self, s: &str) -> Arc<str> {
        if let Some(existing) = self.set.get(s) {
            Arc::clone(existing)
        } else {
            let arc: Arc<str> = Arc::from(s);
            self.set.insert(Arc::clone(&arc));
            arc
        }
    }
}

// ── Internal node state ──────────────────────────────────────────────────────

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
        let mut sorted_children: Vec<([u8; 32], u32)> =
            self.child_counts.into_iter().collect();
        sorted_children.sort_unstable_by(|a, b| a.0.cmp(&b.0));

        let text_digest = if self.has_non_ws_text {
            Some(*self.text_hasher.finalize().as_bytes())
        } else {
            None
        };

        let hash = compute_node_hash(
            &self.name,
            &self.attrs,
            self.text_len,
            text_digest.as_ref(),
            self.child_total,
            &sorted_children,
        );

        let text_short = if self.has_non_ws_text
            && self.text_preview_complete
            && (self.text_len as usize) <= inline_text_max
        {
            Some(self.text_preview)
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

fn compute_node_hash(
    name: &str,
    attrs: &[(String, String)],
    text_len: u64,
    text_digest: Option<&[u8; 32]>,
    child_total: u64,
    sorted_children: &[([u8; 32], u32)],
) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(b"NODE\0");
    h.update(name.as_bytes());
    h.update(b"\0");

    h.update(&(attrs.len() as u64).to_le_bytes());
    for (k, v) in attrs {
        h.update(k.as_bytes());
        h.update(b"\0");
        h.update(v.as_bytes());
        h.update(b"\0");
    }

    match text_digest {
        Some(d) => {
            h.update(&[1]);
            h.update(&text_len.to_le_bytes());
            h.update(d);
        }
        None => {
            h.update(&[0]);
        }
    }

    h.update(&child_total.to_le_bytes());
    h.update(&(sorted_children.len() as u64).to_le_bytes());
    for (child_hash, cnt) in sorted_children {
        h.update(child_hash);
        h.update(&cnt.to_le_bytes());
    }

    *h.finalize().as_bytes()
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub fn build_index(path: &Path, inline_text_max: usize, mode: DiffMode) -> Result<CanonIndex> {
    let file = File::open(path)
        .with_context(|| format!("cannot open {}", path.display()))?;

    // Pre-size from file metadata to reduce resize cascades.
    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let node_hint = ((file_len / 500) as usize).min(4_000_000);
    let attr_hint = ((file_len / 100) as usize).min(20_000_000);

    let mut reader = Reader::from_reader(BufReader::with_capacity(INPUT_BUFFER_CAPACITY, file));
    reader.config_mut().trim_text(false);

    let semantic = mode == DiffMode::Semantic;
    let mut index = CanonIndex {
        totals: Totals::default(),
        nodes: HashMap::with_capacity(node_hint),
        attr_counts: HashMap::with_capacity(attr_hint),
        text_short_counts: HashMap::with_capacity(node_hint / 4),
        text_large_counts: HashMap::with_capacity(node_hint / 16),
    };
    let mut interner = Interner::new();
    let mut path_builder = PathBuilder::new();
    let mut states: Vec<NodeState> = Vec::with_capacity(64);
    let mut skip_depth: u32 = 0;
    let mut buf = Vec::with_capacity(XML_EVENT_BUFFER_CAPACITY);

    loop {
        let event = reader.read_event_into(&mut buf).with_context(|| {
            format!(
                "xml read error in {} at byte {}",
                path.display(),
                reader.buffer_position()
            )
        })?;

        match event {
            Event::Start(ref e) => {
                let name = decode_local_name(e.name().as_ref());

                if semantic && skip_depth > 0 {
                    skip_depth += 1;
                    buf.clear();
                    continue;
                }
                if semantic && tables::is_transport_element(name.as_bytes()) {
                    skip_depth = 1;
                    buf.clear();
                    continue;
                }

                let attrs = parse_attrs_filtered(e, reader.decoder(), semantic)?;

                // Push to path builder and record.
                path_builder.push(&name);
                let path_arc = interner.intern(path_builder.as_str());

                index.totals.nodes += 1;
                index.totals.attrs = index.totals.attrs.saturating_add(attrs.len() as u64);
                record_attrs(&mut index, &mut interner, &path_arc, &attrs);

                states.push(NodeState::new(name, attrs));
            }
            Event::Empty(ref e) => {
                let name = decode_local_name(e.name().as_ref());

                if semantic && skip_depth > 0 {
                    buf.clear();
                    continue;
                }
                if semantic && tables::is_transport_element(name.as_bytes()) {
                    buf.clear();
                    continue;
                }

                let attrs = parse_attrs_filtered(e, reader.decoder(), semantic)?;

                let guard = path_builder.with_leaf(&name);
                let path_arc = interner.intern(guard.as_str());

                index.totals.nodes += 1;
                index.totals.attrs = index.totals.attrs.saturating_add(attrs.len() as u64);
                record_attrs(&mut index, &mut interner, &path_arc, &attrs);

                let node = NodeState::new(name, attrs).finalize(inline_text_max);
                record_final_node(&mut index, &path_arc, &node);
                drop(guard);

                if let Some(parent) = states.last_mut() {
                    parent.absorb_child_hash(node.hash);
                }
            }
            Event::Text(ref t) => {
                if skip_depth > 0 {
                    buf.clear();
                    continue;
                }
                if let Some(curr) = states.last_mut() {
                    let txt = t.decode().with_context(|| {
                        format!("text decode error in {}", path.display())
                    })?;
                    if !txt.trim().is_empty() {
                        curr.absorb_text(txt.as_ref(), inline_text_max);
                    }
                }
            }
            Event::CData(ref c) => {
                if skip_depth > 0 {
                    buf.clear();
                    continue;
                }
                if let Some(curr) = states.last_mut() {
                    let txt = c.decode().with_context(|| {
                        format!("cdata decode error in {}", path.display())
                    })?;
                    if !txt.trim().is_empty() {
                        curr.absorb_text(txt.as_ref(), inline_text_max);
                    }
                }
            }
            Event::End(ref e) => {
                let close_name = decode_local_name(e.name().as_ref());

                if semantic && skip_depth > 0 {
                    skip_depth -= 1;
                    buf.clear();
                    continue;
                }

                let node_state = states
                    .pop()
                    .with_context(|| format!(
                        "unexpected closing tag </{close_name}> in {}",
                        path.display()
                    ))?;

                let path_arc = interner.intern(path_builder.as_str());

                // Verify tag matching (the name on the stack should match).
                if node_state.name != close_name {
                    bail!(
                        "mismatched closing tag in {}: opened <{}> closed </{}>",
                        path.display(),
                        node_state.name,
                        close_name
                    );
                }

                let node = node_state.finalize(inline_text_max);
                record_final_node(&mut index, &path_arc, &node);
                path_builder.pop();

                if let Some(parent) = states.last_mut() {
                    parent.absorb_child_hash(node.hash);
                }
            }
            Event::Eof => break,
            Event::GeneralRef(ref r) => {
                if skip_depth > 0 {
                    buf.clear();
                    continue;
                }
                if let Some(curr) = states.last_mut() {
                    let resolved = resolve_entity_ref(r)?;
                    if !resolved.is_empty() {
                        curr.absorb_text(&resolved, inline_text_max);
                    }
                }
            }
            _ => {}
        }

        buf.clear();
    }

    if !states.is_empty() {
        bail!("unexpected EOF in {} (unclosed tags remaining)", path.display());
    }

    Ok(index)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Resolve an XML entity reference to its string value.
///
/// Handles the 5 predefined XML entities (`amp`, `lt`, `gt`, `quot`, `apos`)
/// and numeric character references (`#x...`, `#...`).
fn resolve_entity_ref(r: &quick_xml::events::BytesRef<'_>) -> Result<String> {
    // Try numeric character reference first (e.g. &#x20; or &#65;).
    match r.resolve_char_ref() {
        Ok(Some(ch)) => return Ok(ch.to_string()),
        Err(e) => anyhow::bail!("invalid character reference: {e}"),
        Ok(None) => {}
    }

    // Named predefined XML entity.
    let name = r.decode().context("entity ref decode failed")?;
    match name.as_ref() {
        "amp" => Ok("&".into()),
        "lt" => Ok("<".into()),
        "gt" => Ok(">".into()),
        "quot" => Ok("\"".into()),
        "apos" => Ok("'".into()),
        other => {
            // Unknown entity — this shouldn't occur in well-formed mzML, but
            // we silently pass it through as `&name;` to avoid data loss.
            Ok(format!("&{other};"))
        }
    }
}

/// Parse attributes, optionally applying semantic filters.
fn parse_attrs_filtered(
    start: &BytesStart,
    decoder: Decoder,
    semantic: bool,
) -> Result<Vec<(String, String)>> {
    let mut attrs = Vec::new();

    for attr in start.attributes().with_checks(false) {
        let attr = attr.context("attribute parse failed")?;
        let raw_key = attr.key.as_ref();

        // Filter xmlns/xmlns:* namespace declarations in semantic mode.
        if semantic && (raw_key == b"xmlns" || raw_key.starts_with(b"xmlns:")) {
            continue;
        }

        let key = decode_local_name(raw_key);
        let value = attr
            .decode_and_unescape_value(decoder)
            .context("attribute value decode failed")?
            .into_owned();

        if semantic && tables::is_transport_attr(key.as_bytes()) {
            continue;
        }

        attrs.push((key, value));
    }

    if semantic {
        normalize_cv_attrs(&mut attrs);
    }

    attrs.sort_unstable();
    Ok(attrs)
}

/// Strip namespace prefix and URI from a qualified name, keeping only the
/// local part.
fn decode_local_name(raw: &[u8]) -> String {
    let mut name = raw;
    // Strip {uri} prefix.
    if name.first() == Some(&b'{')
        && let Some(end) = name.iter().position(|&b| b == b'}')
    {
        name = &name[end + 1..];
    }
    // Strip ns:prefix.
    if let Some(colon) = name.iter().rposition(|&b| b == b':') {
        name = &name[colon + 1..];
    }
    String::from_utf8_lossy(name).into_owned()
}

fn record_attrs(
    index: &mut CanonIndex,
    interner: &mut Interner,
    path: &Arc<str>,
    attrs: &[(String, String)],
) {
    for (k, v) in attrs {
        let key = AttrKey {
            path: Arc::clone(path),
            name: interner.intern(k),
            value: v.clone(),
        };
        *index.attr_counts.entry(key).or_insert(0) += 1;
    }
}

fn record_final_node(index: &mut CanonIndex, path: &Arc<str>, node: &FinalNode) {
    let key = NodeKey {
        path: Arc::clone(path),
        hash: node.hash,
    };

    let entry = index.nodes.entry(key).or_insert_with(|| NodeEntry {
        count: 0,
        preview: node.preview.clone(),
    });
    entry.count += 1;

    if let Some(ref short_text) = node.text_short {
        index.totals.text_nodes += 1;
        index.totals.text_bytes = index
            .totals
            .text_bytes
            .saturating_add(short_text.len() as u64);
        let tk = TextShortKey {
            path: Arc::clone(path),
            value: short_text.clone(),
        };
        *index.text_short_counts.entry(tk).or_insert(0) += 1;
    }

    if let Some((len, digest)) = node.text_large {
        index.totals.text_nodes += 1;
        index.totals.text_bytes = index.totals.text_bytes.saturating_add(len);
        let tk = TextLargeKey {
            path: Arc::clone(path),
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
    use crate::report::format::{clip, hex_prefix};

    let mut out = String::new();
    out.push('<');
    out.push_str(name);

    for (k, v) in attrs.iter().take(PREVIEW_ATTR_LIMIT) {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&clip(v, PREVIEW_VALUE_LIMIT));
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

    clip(&out, PREVIEW_LINE_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_local_name_plain() {
        assert_eq!(decode_local_name(b"spectrum"), "spectrum");
    }

    #[test]
    fn decode_local_name_with_ns_prefix() {
        assert_eq!(decode_local_name(b"ms:spectrum"), "spectrum");
    }

    #[test]
    fn decode_local_name_with_uri() {
        assert_eq!(
            decode_local_name(b"{http://psi.hupo.org/ms/mzml}spectrum"),
            "spectrum"
        );
    }

    #[test]
    fn decode_local_name_with_uri_and_prefix() {
        assert_eq!(
            decode_local_name(b"{http://example.org}ns:elem"),
            "elem"
        );
    }

    #[test]
    fn node_state_absorb_text() {
        let mut ns = NodeState::new("test".into(), vec![]);
        ns.absorb_text("hello", 256);
        assert!(ns.has_non_ws_text);
        assert_eq!(ns.text_len, 5);
        assert_eq!(ns.text_preview, "hello");
        assert!(ns.text_preview_complete);
    }

    #[test]
    fn node_state_absorb_text_exceeds_limit() {
        let mut ns = NodeState::new("test".into(), vec![]);
        ns.absorb_text("hello", 3);
        assert!(!ns.text_preview_complete);
        assert_eq!(ns.text_len, 5);
    }

    #[test]
    fn node_state_child_counting() {
        let mut ns = NodeState::new("test".into(), vec![]);
        let hash = [0u8; 32];
        ns.absorb_child_hash(hash);
        ns.absorb_child_hash(hash);
        assert_eq!(ns.child_total, 2);
        assert_eq!(ns.child_counts[&hash], 2);
    }

    #[test]
    fn finalize_empty_node() {
        let ns = NodeState::new("empty".into(), vec![]);
        let f = ns.finalize(256);
        assert!(f.text_short.is_none());
        assert!(f.text_large.is_none());
        assert!(f.preview.descriptor.contains("<empty>"));
    }

    #[test]
    fn finalize_node_with_short_text() {
        let mut ns = NodeState::new("val".into(), vec![]);
        ns.absorb_text("hello", 256);
        let f = ns.finalize(256);
        assert_eq!(f.text_short.as_deref(), Some("hello"));
        assert!(f.text_large.is_none());
    }

    #[test]
    fn finalize_node_with_large_text() {
        let mut ns = NodeState::new("val".into(), vec![]);
        let big = "x".repeat(300);
        ns.absorb_text(&big, 256);
        let f = ns.finalize(256);
        assert!(f.text_short.is_none());
        assert!(f.text_large.is_some());
        let (len, _digest) = f.text_large.unwrap();
        assert_eq!(len, 300);
    }

    #[test]
    fn compute_hash_deterministic() {
        let attrs = vec![("a".into(), "1".into())];
        let h1 = compute_node_hash("x", &attrs, 0, None, 0, &[]);
        let h2 = compute_node_hash("x", &attrs, 0, None, 0, &[]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn compute_hash_differs_on_name() {
        let h1 = compute_node_hash("a", &[], 0, None, 0, &[]);
        let h2 = compute_node_hash("b", &[], 0, None, 0, &[]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn compute_hash_differs_on_attrs() {
        let a1 = vec![("k".into(), "v1".into())];
        let a2 = vec![("k".into(), "v2".into())];
        let h1 = compute_node_hash("x", &a1, 0, None, 0, &[]);
        let h2 = compute_node_hash("x", &a2, 0, None, 0, &[]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn compute_hash_differs_on_text() {
        let d1 = [1u8; 32];
        let d2 = [2u8; 32];
        let h1 = compute_node_hash("x", &[], 10, Some(&d1), 0, &[]);
        let h2 = compute_node_hash("x", &[], 10, Some(&d2), 0, &[]);
        assert_ne!(h1, h2);
    }
}
