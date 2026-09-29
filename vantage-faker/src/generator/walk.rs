//! `Walk`: a clamped random walk indexed by `seq`. Steps come from
//! [`hash::unit`], so the series is a pure function of the salt; it is
//! extended on demand and memoised, making any row O(1) after the first
//! build of a table.

use super::hash::{self, STREAM_WALK};

#[derive(Debug)]
pub(crate) struct Params {
    pub start: f64,
    pub step: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl Params {
    fn clamp(&self, v: f64) -> f64 {
        let v = self.min.map_or(v, |lo| v.max(lo));
        self.max.map_or(v, |hi| v.min(hi))
    }
}

/// Value at `seq`, growing `series` (the memo for these params) as needed.
pub(crate) fn value_at(series: &mut Vec<f64>, params: &Params, salt: u64, seq: usize) -> f64 {
    if series.is_empty() {
        series.push(params.clamp(params.start));
    }
    while series.len() <= seq {
        let i = series.len() as u64;
        let delta = (hash::unit(salt, i, STREAM_WALK) * 2.0 - 1.0) * params.step;
        let next = params.clamp(series[series.len() - 1] + delta);
        series.push(next);
    }
    series[seq]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Params {
        Params {
            start: 50.0,
            step: 10.0,
            min: Some(0.0),
            max: Some(60.0),
        }
    }

    #[test]
    fn walk_stays_clamped() {
        let mut series = Vec::new();
        for seq in 0..2000 {
            let v = value_at(&mut series, &params(), 7, seq);
            assert!((0.0..=60.0).contains(&v), "row {seq}: {v}");
        }
    }

    #[test]
    fn walk_row_does_not_depend_on_access_order() {
        let mut forward = Vec::new();
        let expected: Vec<f64> = (0..600)
            .map(|s| value_at(&mut forward, &params(), 7, s))
            .collect();
        let mut jump = Vec::new();
        assert_eq!(value_at(&mut jump, &params(), 7, 500), expected[500]);
        assert_eq!(value_at(&mut jump, &params(), 7, 3), expected[3]);
    }
}
