//! `Tree`: same-table parent assignment, laid out breadth-first.
//!
//! For `n` rows, `roots` roots and `depth` levels the plan picks a branching
//! factor `b` so that `roots · (1 + b + … + b^(depth-1)) ≈ n`, fills level by
//! level in `seq` order, and spreads each level evenly across the one above
//! with a little jitter — fan-out varies around `b`, and no parent hoards the
//! children the way a uniformly random parent pick would.

use super::hash::{self, STREAM_TREE};

pub(crate) struct Plan {
    /// Per row: parent seq (`None` for roots) and level (roots are 0).
    rows: Vec<(Option<usize>, u8)>,
    depth: u8,
}

impl Plan {
    pub(crate) fn new(roots: usize, depth: u8, count: usize, salt: u64) -> Self {
        let depth = depth.max(1);
        let roots = roots.max(1);
        let mut rows = Vec::with_capacity(count);
        let mut prev = 0..0;
        for (level, size) in level_sizes(roots, depth, count).into_iter().enumerate() {
            let start = rows.len();
            for j in 0..size {
                let parent = (level > 0).then(|| {
                    let b = size as f64 / prev.len() as f64;
                    let u = hash::unit(salt, (start + j) as u64, STREAM_TREE);
                    let pos = (j as f64 + 0.5 + (u - 0.5) * 0.8 * b.max(1.0)) / b;
                    prev.start + (pos.max(0.0) as usize).min(prev.len() - 1)
                });
                rows.push((parent, level as u8));
            }
            prev = start..rows.len();
        }
        Self { rows, depth }
    }

    /// Parent of `seq`. Rows past the planned count attach to a hashed
    /// planned row that still has room below it, so they never add a level.
    pub(crate) fn parent_of(&self, seq: usize, salt: u64) -> Option<usize> {
        if let Some((parent, _)) = self.rows.get(seq) {
            return *parent;
        }
        if self.depth == 1 || self.rows.is_empty() {
            return None;
        }
        let r = (hash::mix(salt, seq as u64, STREAM_TREE) % self.rows.len() as u64) as usize;
        // Roots sit at the front and always have room, so this terminates.
        (0..=r).rev().find(|&i| self.rows[i].1 + 1 < self.depth)
    }
}

/// Rows per level: a geometric progression summing to `count`, with trailing
/// empty levels when `count` is too small to reach `depth`.
fn level_sizes(roots: usize, depth: u8, count: usize) -> Vec<usize> {
    let levels = usize::from(depth);
    if levels == 1 || count <= roots {
        return vec![count];
    }
    let total = |b: f64| {
        (0..levels)
            .map(|k| roots as f64 * b.powi(k as i32))
            .sum::<f64>()
    };
    let (mut lo, mut hi) = (1.0_f64, count as f64);
    for _ in 0..64 {
        let mid = (lo + hi) / 2.0;
        if total(mid) < count as f64 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let mut sizes = Vec::with_capacity(levels);
    let (mut cum, mut acc) = (0usize, 0.0_f64);
    for k in 0..levels {
        acc += roots as f64 * hi.powi(k as i32);
        let next = if k + 1 == levels {
            count
        } else {
            (acc.round() as usize).min(count)
        };
        sizes.push(next - cum);
        cum = next;
    }
    sizes.retain(|&s| s > 0);
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth_of(plan: &Plan, mut seq: usize) -> u8 {
        let mut d = 1;
        while let Some(p) = plan.parent_of(seq, 5) {
            assert!(p < seq, "parent {p} not below {seq}");
            seq = p;
            d += 1;
        }
        d
    }

    #[test]
    fn tree_has_roots_ordered_parents_and_bounded_depth() {
        for (roots, depth, count) in [(5, 3, 200), (1, 4, 1000), (3, 2, 10), (4, 5, 30)] {
            let plan = Plan::new(roots, depth, count, 5);
            for seq in 0..count {
                let parent = plan.parent_of(seq, 5);
                assert_eq!(parent.is_none(), seq < roots, "row {seq}");
                assert!(depth_of(&plan, seq) <= depth);
            }
            // Overflow rows keep the invariants too.
            for seq in count..count + 50 {
                assert!(plan.parent_of(seq, 5).is_some());
                assert!(depth_of(&plan, seq) <= depth);
            }
        }
    }

    #[test]
    fn tree_is_fairly_balanced() {
        let plan = Plan::new(5, 3, 155, 5); // b = 5 → 5 + 25 + 125
        let mut children = vec![0usize; 155];
        for seq in 0..155 {
            if let Some(p) = plan.parent_of(seq, 5) {
                children[p] += 1;
            }
        }
        let level1: Vec<_> = (0..155).filter(|&s| plan.rows[s].1 == 1).collect();
        assert_eq!(level1.len(), 25);
        let max = level1.iter().map(|&s| children[s]).max().unwrap();
        assert!(max <= 12, "a parent hoards {max} children");
    }

    #[test]
    fn depth_one_makes_every_row_a_root() {
        let plan = Plan::new(2, 1, 20, 5);
        assert!((0..40).all(|s| plan.parent_of(s, 5).is_none()));
    }
}
