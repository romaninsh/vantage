//! `Tree`: same-table parent assignment, laid out breadth-first.
//!
//! For `n` rows, `roots` roots and `depth` levels the plan picks a branching
//! factor `b` so that `roots · (1 + b + … + b^(depth-1)) ≈ n`, fills level by
//! level in `seq` order, and spreads each level evenly across the one above
//! with a little jitter — fan-out varies around `b`, and no parent hoards the
//! children the way a uniformly random parent pick would.
//!
//! With a `min_depth`, rows past that level may end their branch (see
//! [`levels::Ends`]); the next level spreads only across rows that did not.

mod levels;

use super::hash::{self, STREAM_TREE, STREAM_TREE_END};
use levels::{Ends, level_sizes};

pub(crate) struct Plan {
    rows: Vec<Row>,
    depth: u8,
}

struct Row {
    /// Parent seq, `None` for roots.
    parent: Option<usize>,
    /// Roots are level 0.
    level: u8,
    /// Whether the plan lets this row have children.
    room: bool,
}

impl Plan {
    pub(crate) fn new(
        roots: usize,
        depth: u8,
        min_depth: Option<u8>,
        count: usize,
        salt: u64,
    ) -> Self {
        let ends = Ends::new(depth.max(1), min_depth);
        let roots = roots.max(1);
        let mut rows: Vec<Row> = Vec::with_capacity(count);
        let mut prev = 0..0;
        for (level, size) in level_sizes(roots, &ends, count).into_iter().enumerate() {
            let level = level as u8;
            let start = rows.len();
            let mut open: Vec<usize> = prev.clone().filter(|&i| rows[i].room).collect();
            if open.is_empty() {
                open = prev.clone().collect();
            }
            // Rows above `min_depth` must branch; without jitter a level at
            // least as large as the one above leaves none of them childless.
            let jitter = if level > 0 && ends.forced(level - 1) {
                0.0
            } else {
                0.8
            };
            for j in 0..size {
                let seq = (start + j) as u64;
                let parent = (level > 0).then(|| {
                    let b = size as f64 / open.len() as f64;
                    let u = hash::unit(salt, seq, STREAM_TREE);
                    let pos = (j as f64 + 0.5 + (u - 0.5) * jitter * b.max(1.0)) / b;
                    open[(pos.max(0.0) as usize).min(open.len() - 1)]
                });
                let room = ends.room(level, hash::unit(salt, seq, STREAM_TREE_END));
                rows.push(Row {
                    parent,
                    level,
                    room,
                });
            }
            prev = start..rows.len();
        }
        Self {
            rows,
            depth: ends.depth,
        }
    }

    /// Parent of `seq`. Rows past the planned count attach to a hashed
    /// planned row that still has room below it, so they never add a level.
    pub(crate) fn parent_of(&self, seq: usize, salt: u64) -> Option<usize> {
        if let Some(row) = self.rows.get(seq) {
            return row.parent;
        }
        if self.depth == 1 || self.rows.is_empty() {
            return None;
        }
        let r = (hash::mix(salt, seq as u64, STREAM_TREE) % self.rows.len() as u64) as usize;
        // Roots sit at the front and are never on the last level, so the
        // fallback terminates.
        (0..=r)
            .rev()
            .find(|&i| self.rows[i].room)
            .or_else(|| (0..=r).rev().find(|&i| self.rows[i].level + 1 < self.depth))
    }
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
            let plan = Plan::new(roots, depth, None, count, 5);
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
        let plan = Plan::new(5, 3, None, 155, 5); // b = 5 → 5 + 25 + 125
        let mut children = vec![0usize; 155];
        for seq in 0..155 {
            if let Some(p) = plan.parent_of(seq, 5) {
                children[p] += 1;
            }
        }
        let level1: Vec<_> = (0..155).filter(|&s| plan.rows[s].level == 1).collect();
        assert_eq!(level1.len(), 25);
        let max = level1.iter().map(|&s| children[s]).max().unwrap();
        assert!(max <= 12, "a parent hoards {max} children");
    }

    #[test]
    fn depth_one_makes_every_row_a_root() {
        let plan = Plan::new(2, 1, None, 20, 5);
        assert!((0..40).all(|s| plan.parent_of(s, 5).is_none()));
    }

    #[test]
    fn min_depth_ends_branches_between_min_and_depth() {
        let (depth, min, count) = (5, 3, 3000);
        let plan = Plan::new(5, depth, Some(min), count, 5);
        let mut children = vec![0usize; count];
        for seq in 0..count {
            if let Some(p) = plan.parent_of(seq, 5) {
                children[p] += 1;
            }
        }
        let mut end_levels = std::collections::BTreeSet::new();
        for (seq, &n) in children.iter().enumerate() {
            let d = depth_of(&plan, seq);
            assert!(d <= depth, "row {seq} at level {d}");
            if n == 0 {
                assert!(d >= min, "row {seq} ends its branch at level {d}");
                end_levels.insert(d);
            }
        }
        assert_eq!(end_levels.into_iter().collect::<Vec<_>>(), vec![3, 4, 5]);
        for seq in count..count + 50 {
            assert!(depth_of(&plan, seq) <= depth);
        }
        // The same salt repeats the plan.
        let again = Plan::new(5, depth, Some(min), count, 5);
        assert!((0..count).all(|s| plan.parent_of(s, 5) == again.parent_of(s, 5)));
    }
}
