/// A reusable, stack-based path builder that avoids allocations during XML
/// traversal. Tracks segment boundaries so `push` / `pop` are O(1).
///
/// The materialised path is always available via `as_str()` and looks like
/// `/mzML/run/spectrumList/spectrum`.
pub struct PathBuilder {
    buf: String,
    /// Byte offsets where each segment starts (including the leading `/`).
    offsets: Vec<usize>,
}

impl PathBuilder {
    pub fn new() -> Self {
        Self {
            buf: String::with_capacity(256),
            offsets: Vec::with_capacity(32),
        }
    }

    /// Append a new path segment: `"/name"`.
    pub fn push(&mut self, name: &str) {
        self.offsets.push(self.buf.len());
        self.buf.push('/');
        self.buf.push_str(name);
    }

    /// Remove the last segment, restoring the buffer to the parent path.
    pub fn pop(&mut self) {
        if let Some(offset) = self.offsets.pop() {
            self.buf.truncate(offset);
        }
    }

    /// The current path, including a temporarily-appended leaf.
    /// This is a convenience for elements that open + close in one shot
    /// (`Event::Empty`) without modifying the permanent stack.
    pub fn with_leaf<'a>(&'a mut self, leaf: &str) -> PathGuard<'a> {
        let restore_len = self.buf.len();
        self.buf.push('/');
        self.buf.push_str(leaf);
        PathGuard {
            builder: self,
            restore_len,
        }
    }

    /// Current path as `&str`. Returns `"/"` when empty.
    pub fn as_str(&self) -> &str {
        if self.buf.is_empty() {
            "/"
        } else {
            &self.buf
        }
    }
}

impl Default for PathBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard that appends a temporary leaf segment to a [`PathBuilder`] and
/// restores the original length on drop.
pub struct PathGuard<'a> {
    builder: &'a mut PathBuilder,
    restore_len: usize,
}

impl PathGuard<'_> {
    pub fn as_str(&self) -> &str {
        if self.builder.buf.is_empty() {
            "/"
        } else {
            &self.builder.buf
        }
    }
}

impl Drop for PathGuard<'_> {
    fn drop(&mut self) {
        self.builder.buf.truncate(self.restore_len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_path_is_root() {
        let pb = PathBuilder::new();
        assert_eq!(pb.as_str(), "/");
    }

    #[test]
    fn push_and_pop() {
        let mut pb = PathBuilder::new();
        pb.push("mzML");
        assert_eq!(pb.as_str(), "/mzML");

        pb.push("run");
        assert_eq!(pb.as_str(), "/mzML/run");

        pb.push("spectrumList");
        assert_eq!(pb.as_str(), "/mzML/run/spectrumList");

        pb.pop();
        assert_eq!(pb.as_str(), "/mzML/run");

        pb.pop();
        assert_eq!(pb.as_str(), "/mzML");

        pb.pop();
        assert_eq!(pb.as_str(), "/");
    }

    #[test]
    fn with_leaf_restores_on_drop() {
        let mut pb = PathBuilder::new();
        pb.push("mzML");
        pb.push("run");

        {
            let guard = pb.with_leaf("spectrum");
            assert_eq!(guard.as_str(), "/mzML/run/spectrum");
        }
        // Guard dropped — back to parent.
        assert_eq!(pb.as_str(), "/mzML/run");
    }

    #[test]
    fn with_leaf_on_empty_path() {
        let mut pb = PathBuilder::new();
        {
            let guard = pb.with_leaf("root");
            assert_eq!(guard.as_str(), "/root");
        }
        assert_eq!(pb.as_str(), "/");
    }

    #[test]
    fn pop_on_empty_is_noop() {
        let mut pb = PathBuilder::new();
        pb.pop();
        assert_eq!(pb.as_str(), "/");
    }

    #[test]
    fn push_pop_push_reuses_buffer() {
        let mut pb = PathBuilder::new();
        pb.push("a");
        pb.push("very_long_segment_name");
        let cap_before = pb.buf.capacity();

        pb.pop();
        pb.pop();
        pb.push("b");

        // The buffer capacity shouldn't have shrunk — we reuse the allocation.
        assert!(pb.buf.capacity() >= cap_before);
        assert_eq!(pb.as_str(), "/b");
    }
}
