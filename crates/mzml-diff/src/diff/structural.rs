use std::hash::Hash;

use hashbrown::HashMap;

/// Trait for values that carry a count. Allows `diff_counts` to work with
/// both plain `u64` maps and merged `NodeEntry` maps.
pub trait Counted {
    fn count(&self) -> u64;
}

impl Counted for u64 {
    #[inline]
    fn count(&self) -> u64 {
        *self
    }
}

/// A single counted difference between left and right for a key `K`.
#[derive(Debug, Clone)]
pub struct Delta<K> {
    pub key: K,
    pub left: u64,
    pub right: u64,
}

/// Compute the symmetric set of deltas between two count maps.
pub fn diff_counts<K, V>(left: &HashMap<K, V>, right: &HashMap<K, V>) -> Vec<Delta<K>>
where
    K: Eq + Hash + Clone,
    V: Counted,
{
    let mut out = Vec::new();

    for (k, lv) in left {
        let lc = lv.count();
        let rc = right.get(k).map_or(0, |v| v.count());
        if lc != rc {
            out.push(Delta {
                key: k.clone(),
                left: lc,
                right: rc,
            });
        }
    }

    for (k, rv) in right {
        if !left.contains_key(k) {
            out.push(Delta {
                key: k.clone(),
                left: 0,
                right: rv.count(),
            });
        }
    }

    out
}

/// Sort deltas by absolute magnitude (descending), then path, then tie-breaker.
pub fn sort_deltas<K, P, T>(deltas: &mut [Delta<K>], path_key: P, tie_key: T)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_counts_identical_maps() {
        let mut m = HashMap::new();
        m.insert("a", 5);
        m.insert("b", 3);
        assert!(diff_counts(&m, &m).is_empty());
    }

    #[test]
    fn diff_counts_detects_mismatches() {
        let mut left = HashMap::new();
        left.insert("a", 5);
        left.insert("b", 3);

        let mut right = HashMap::new();
        right.insert("a", 5);
        right.insert("b", 4);

        let deltas = diff_counts(&left, &right);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].key, "b");
        assert_eq!(deltas[0].left, 3);
        assert_eq!(deltas[0].right, 4);
    }

    #[test]
    fn diff_counts_left_only() {
        let mut left = HashMap::new();
        left.insert("a", 1);
        let right = HashMap::new();

        let deltas = diff_counts(&left, &right);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].left, 1);
        assert_eq!(deltas[0].right, 0);
    }

    #[test]
    fn diff_counts_right_only() {
        let left = HashMap::new();
        let mut right = HashMap::new();
        right.insert("z", 7);

        let deltas = diff_counts(&left, &right);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].left, 0);
        assert_eq!(deltas[0].right, 7);
    }

    #[test]
    fn sort_deltas_by_magnitude() {
        let mut deltas = vec![
            Delta {
                key: "small",
                left: 1,
                right: 2,
            },
            Delta {
                key: "big",
                left: 0,
                right: 100,
            },
            Delta {
                key: "med",
                left: 5,
                right: 15,
            },
        ];

        sort_deltas(&mut deltas, |k| k, |_| "");

        assert_eq!(deltas[0].key, "big");
        assert_eq!(deltas[1].key, "med");
        assert_eq!(deltas[2].key, "small");
    }
}
