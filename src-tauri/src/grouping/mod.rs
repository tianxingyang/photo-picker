//! Pure near-duplicate clustering over pHash values (no DB, no sidecar).
//!
//! Connected-component / single-link grouping: photos whose 64-bit pHashes are
//! within `threshold` Hamming distance are unioned into one component
//! (transitive: A~B, B~C => {A,B,C}). Only components of size >= 2 are returned;
//! isolated photos are dropped (UI's "ungrouped" bucket). Output is fully
//! deterministic — independent of input order — which is the basis for the
//! command layer's content-derived, idempotent group ids.

/// Parse a 16-hex-char pHash string into a `u64`. Returns `None` for any string
/// that isn't exactly 16 hex digits, so the caller skips unanalysable rows
/// rather than crashing.
pub fn parse_phash(s: &str) -> Option<u64> {
    // why: length guard before from_str_radix — a short hex like "ff" parses
    // fine but is not a valid 64-bit pHash, so reject anything not exactly 16.
    if s.len() != 16 {
        return None;
    }
    u64::from_str_radix(s, 16).ok()
}

/// Hamming distance between two 64-bit pHashes = popcount of their XOR.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Union-find with path compression + union by rank.
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        // Iterative path compression: point every node on the path at the root.
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            std::cmp::Ordering::Less => self.parent[ra] = rb,
            std::cmp::Ordering::Greater => self.parent[rb] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb] = ra;
                self.rank[ra] += 1;
            }
        }
    }
}

/// Cluster photos by pHash Hamming distance into near-duplicate groups.
///
/// Every pair `i < j` with `hamming(h_i, h_j) <= threshold` is unioned. Returns
/// only connected components of size >= 2. Within each component member ids are
/// sorted ascending, and components are ordered by their smallest member id, so
/// the result is identical regardless of input order.
pub fn cluster(items: &[(String, u64)], threshold: u32) -> Vec<Vec<String>> {
    let n = items.len();
    let mut uf = UnionFind::new(n);
    // why: O(n^2) popcount is fine at M1 scale (hundreds~thousands); LSH bucketing
    // is deferred to M2/M3 if scale grows.
    for i in 0..n {
        for j in (i + 1)..n {
            if hamming(items[i].1, items[j].1) <= threshold {
                uf.union(i, j);
            }
        }
    }

    // Group member ids by component root.
    use std::collections::HashMap;
    let mut comps: HashMap<usize, Vec<String>> = HashMap::new();
    for (i, (id, _)) in items.iter().enumerate() {
        let root = uf.find(i);
        comps.entry(root).or_default().push(id.clone());
    }

    let mut result: Vec<Vec<String>> = comps
        .into_values()
        .filter(|members| members.len() >= 2)
        .map(|mut members| {
            members.sort();
            members
        })
        .collect();
    // Order components by smallest (first, since sorted) member id => deterministic.
    result.sort_by(|a, b| a[0].cmp(&b[0]));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_phash_accepts_valid_16_hex() {
        assert_eq!(parse_phash("ffc3a18000000000"), Some(0xffc3_a180_0000_0000));
        assert_eq!(parse_phash("0000000000000000"), Some(0));
    }

    #[test]
    fn parse_phash_rejects_bad_input() {
        assert_eq!(parse_phash(""), None);
        assert_eq!(parse_phash("ff"), None, "too short");
        assert_eq!(parse_phash("ffc3a180000000000"), None, "17 chars, too long");
        assert_eq!(parse_phash("zzzzzzzzzzzzzzzz"), None, "non-hex garbage");
        assert_eq!(parse_phash("ffc3a1800000000g"), None, "trailing non-hex");
    }

    #[test]
    fn hamming_counts_differing_bits() {
        assert_eq!(hamming(0, 0), 0);
        assert_eq!(hamming(0b0000, 0b1011), 3);
        assert_eq!(hamming(u64::MAX, 0), 64);
    }

    #[test]
    fn pair_within_threshold_groups_together() {
        // distance 2 (two bits differ), threshold 8 => same group.
        let items = vec![("a".to_string(), 0b0000u64), ("b".to_string(), 0b0011u64)];
        let groups = cluster(&items, 8);
        assert_eq!(groups, vec![vec!["a".to_string(), "b".to_string()]]);
    }

    #[test]
    fn transitive_closure_merges_chain() {
        // Genuine transitivity: A~B and B~C are each within threshold, but A~C is
        // strictly beyond it, so A and C can only land in one component *via* B.
        let a = 0x0000_0000_0000_0000u64;
        let b = 0x0000_0000_0000_000Fu64; // hamming(a,b) = 4 (<= 8)
        let c = 0x0000_0000_0000_1F0Fu64; // hamming(b,c) = 5 (<= 8)
        debug_assert_eq!(hamming(a, b), 4);
        debug_assert_eq!(hamming(b, c), 5);
        debug_assert_eq!(hamming(a, c), 9, "endpoints A~C exceed threshold 8");
        let items = vec![
            ("c".to_string(), c),
            ("a".to_string(), a),
            ("b".to_string(), b),
        ];
        let groups = cluster(&items, 8);
        assert_eq!(
            groups,
            vec![vec!["a".to_string(), "b".to_string(), "c".to_string()]],
            "A~B~C transitive closure forms one group even though A~C alone exceeds the threshold"
        );
    }

    #[test]
    fn beyond_threshold_splits() {
        // distance 12 > threshold 8 => two separate (size-1) components, neither
        // returned because only size >= 2 is kept.
        let items = vec![
            ("a".to_string(), 0x0000_0000_0000_0000u64),
            ("b".to_string(), 0x0000_0000_0000_0FFFu64), // 12 bits
        ];
        let groups = cluster(&items, 8);
        assert!(groups.is_empty(), "no pair within threshold => no groups");
    }

    #[test]
    fn isolated_photo_absent_from_output() {
        let items = vec![
            ("a".to_string(), 0b0000u64),
            ("b".to_string(), 0b0011u64),     // within 8 of a
            ("lonely".to_string(), u64::MAX), // far from both
        ];
        let groups = cluster(&items, 8);
        assert_eq!(groups, vec![vec!["a".to_string(), "b".to_string()]]);
        assert!(
            !groups.iter().any(|g| g.contains(&"lonely".to_string())),
            "isolated photo must not appear in any component"
        );
    }

    #[test]
    fn shuffled_input_yields_identical_partition() {
        let a = 0x0000_0000_0000_0000u64;
        let b = 0x0000_0000_0000_0003u64; // ~a
        let c = 0xFFFF_FFFF_FFFF_FFFFu64;
        let d = 0xFFFF_FFFF_FFFF_FFFCu64; // ~c

        let order1 = vec![
            ("a".to_string(), a),
            ("b".to_string(), b),
            ("c".to_string(), c),
            ("d".to_string(), d),
        ];
        let order2 = vec![
            ("d".to_string(), d),
            ("a".to_string(), a),
            ("c".to_string(), c),
            ("b".to_string(), b),
        ];

        let g1 = cluster(&order1, 8);
        let g2 = cluster(&order2, 8);
        assert_eq!(g1, g2, "partition is independent of input order");
        assert_eq!(
            g1,
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string(), "d".to_string()],
            ]
        );
    }
}
