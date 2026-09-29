//! Where a tree's branches end, and how many rows each level holds.

/// Branch-end rule of a tree plan. Rows on the last level (`depth`) never
/// have children. Without `min`, every other row may have them. With
/// `min`, rows above level `min` always have children, and a row between
/// the two stops its branch with odds `1 / (levels left)`, which spreads a
/// path's end evenly over `min..=depth`.
#[derive(Clone, Copy)]
pub(super) struct Ends {
    pub depth: u8,
    min: Option<u8>,
}

impl Ends {
    pub fn new(depth: u8, min: Option<u8>) -> Self {
        Self {
            depth,
            min: min.map(|m| m.clamp(1, depth)),
        }
    }

    /// Whether every row on 0-based `level` must have children.
    pub fn forced(&self, level: u8) -> bool {
        self.min.is_some_and(|m| level + 1 < m)
    }

    /// Chance that a row on 0-based `level` may have children.
    fn keep(&self, level: u8) -> f64 {
        if level + 1 >= self.depth {
            0.0
        } else if self.min.is_none() || self.forced(level) {
            1.0
        } else {
            1.0 - 1.0 / f64::from(self.depth - level)
        }
    }

    /// Whether a row on `level` with draw `u` in `[0, 1)` may have children.
    pub fn room(&self, level: u8, u: f64) -> bool {
        u < self.keep(level)
    }
}

/// Rows per level: `roots · w_k · b^k` summing to `count`, where `w_k` is
/// the expected share of level `k` whose branch has not ended above it.
/// Trailing empty levels are dropped when `count` is too small to reach
/// `depth`.
pub(super) fn level_sizes(roots: usize, ends: &Ends, count: usize) -> Vec<usize> {
    let levels = usize::from(ends.depth);
    if levels == 1 || count <= roots {
        return vec![count];
    }
    let mut weights = Vec::with_capacity(levels);
    let mut w = 1.0_f64;
    for k in 0..levels {
        weights.push(w);
        w *= ends.keep(k as u8);
    }
    let size = |k: usize, b: f64| roots as f64 * weights[k] * b.powi(k as i32);
    let total = |b: f64| (0..levels).map(|k| size(k, b)).sum::<f64>();
    // The last weight is the smallest; dividing by it keeps `total(hi)`
    // above `count`.
    let (mut lo, mut hi) = (1.0_f64, count as f64 / weights[levels - 1]);
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
        acc += size(k, hi);
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
