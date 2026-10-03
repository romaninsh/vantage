//! Random verbs, drawn from the sim's own rng.

use fake::rand::RngExt as _;
use fake::rand::rngs::StdRng;
use vantage_rhai::rhai::{Array, Dynamic, Engine};

use super::num;
use crate::FakerColumn;
use crate::generator::{expand_pattern, parse_when, rfc3339, sentence};
use crate::sim::current::{VerbResult, with};
use crate::value_gen::ValueGen;
use vantage_vista::rhai::cbor_to_dynamic;

/// Index into `weights` drawn in proportion to them.
fn weighted(rng: &mut StdRng, weights: &[f64]) -> VerbResult<usize> {
    let sum: f64 = weights.iter().sum();
    if weights.iter().any(|w| !w.is_finite() || *w < 0.0) || !sum.is_finite() || sum <= 0.0 {
        return Err("pick_weighted: weights must be finite, not negative, and not all zero".into());
    }
    let mut r = rng.random_range(0.0..sum);
    Ok(weights
        .iter()
        .position(|w| {
            r -= w;
            r < 0.0
        })
        .unwrap_or(weights.len() - 1))
}

pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("pick", |arr: Array| {
        with(|c| {
            Ok(match arr.len() {
                0 => Dynamic::UNIT,
                n => arr[c.rng.random_range(0..n)].clone(),
            })
        })
    });

    engine.register_fn("pick_weighted", |arr: Array, weights: Array| {
        with(|c| {
            if arr.len() != weights.len() || arr.is_empty() {
                return Err("pick_weighted: needs as many weights as values, and a value".into());
            }
            let w = weights.iter().map(num).collect::<VerbResult<Vec<f64>>>()?;
            Ok(arr[weighted(&mut c.rng, &w)?].clone())
        })
    });

    engine.register_fn("rand_int", |lo: i64, hi: i64| {
        with(|c| Ok(c.rng.random_range(lo.min(hi)..=lo.max(hi))))
    });

    engine.register_fn("rand_float", |lo: Dynamic, hi: Dynamic| {
        with(|c| {
            let (lo, hi) = (num(&lo)?, num(&hi)?);
            if !lo.is_finite() || !hi.is_finite() {
                return Err("rand_float: bounds must be finite".into());
            }
            Ok(if hi > lo {
                c.rng.random_range(lo..hi)
            } else {
                lo
            })
        })
    });

    engine.register_fn("chance", |p: Dynamic| {
        with(|c| {
            let p = num(&p)?;
            Ok(p.is_finite() && c.rng.random_range(0.0..1.0) < p)
        })
    });

    engine.register_fn("pattern", |template: &str| {
        with(|c| Ok(expand_pattern(&mut c.rng, template)))
    });

    engine.register_fn("sentence", |min: i64, max: i64| {
        with(|c| {
            let clamp = |n: i64| n.clamp(1, 255) as u8;
            Ok(cbor_to_dynamic(&sentence(
                &mut c.rng,
                clamp(min),
                clamp(max),
            )))
        })
    });

    engine.register_fn("fake", |kind: &str| {
        with(|c| {
            let col = FakerColumn::new(kind, kind);
            Ok(cbor_to_dynamic(&ValueGen::value_for_with(&mut c.rng, &col)))
        })
    });

    engine.register_fn("date_between", |from: &str, to: &str| {
        with(|c| {
            let now = c.vt as i64;
            let parse = |s: &str| {
                parse_when(s, now).ok_or_else(|| format!("date_between: cannot read date {s:?}"))
            };
            let (a, b) = (parse(from)?, parse(to)?);
            let (lo, hi) = (a.min(b), a.max(b));
            Ok(rfc3339(c.rng.random_range(lo..=hi)))
        })
    });
}
