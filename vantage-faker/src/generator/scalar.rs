//! Stream generators that need no calendar or memo: pick, range, sentence,
//! plus the shared number formatting.

use ciborium::Value as CborValue;
use fake::Fake;
use fake::faker::lorem::en::Word;
use fake::rand::RngExt as _;
use fake::rand::rngs::StdRng;

/// Round to `decimals` places; `None`/0 yields an integer.
pub(crate) fn number(v: f64, decimals: Option<u8>) -> CborValue {
    match decimals {
        None | Some(0) => CborValue::Integer((v.round() as i64).into()),
        Some(d) => {
            let f = 10f64.powi(i32::from(d.min(15)));
            CborValue::Float((v * f).round() / f)
        }
    }
}

pub(crate) fn range(rng: &mut StdRng, min: f64, max: f64, decimals: Option<u8>) -> CborValue {
    if !min.is_finite() || !max.is_finite() {
        // `validate()` should have caught this; a raw config that skips it
        // has no meaningful draw to make, so fall back to whichever bound is
        // usable instead of handing rand an empty/infinite range.
        let fallback = if min.is_finite() {
            min
        } else if max.is_finite() {
            max
        } else {
            0.0
        };
        return number(fallback, decimals);
    }
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    match decimals {
        None | Some(0) => {
            let (a, b) = (lo.ceil() as i64, hi.floor() as i64);
            let n = if a <= b {
                rng.random_range(a..=b)
            } else {
                lo.round() as i64
            };
            CborValue::Integer(n.into())
        }
        Some(_) => {
            let v = if lo < hi {
                rng.random_range(lo..=hi)
            } else {
                lo
            };
            // Rounding can step just past a bound; pull it back in.
            match number(v, decimals) {
                CborValue::Float(f) => CborValue::Float(f.clamp(lo, hi)),
                other => other,
            }
        }
    }
}

pub(crate) fn pick(
    rng: &mut StdRng,
    values: &[String],
    weights: Option<&[f64]>,
    ty: &str,
) -> CborValue {
    if values.is_empty() {
        return CborValue::Null;
    }
    // A weight set only drives the draw when every weight is finite and
    // non-negative and the total neither is zero nor overflows to infinity —
    // any of those would hand rand an empty or non-finite range to draw from.
    let weights = weights.filter(|w| w.len() == values.len()).and_then(|w| {
        let sum = w.iter().sum::<f64>();
        (w.iter().all(|x| x.is_finite() && *x >= 0.0) && sum.is_finite() && sum > 0.0)
            .then_some((w, sum))
    });
    let idx = match weights {
        Some((w, sum)) => {
            let mut r = rng.random_range(0.0..sum);
            w.iter()
                .position(|x| {
                    r -= x;
                    r < 0.0
                })
                .unwrap_or(values.len() - 1)
        }
        None => rng.random_range(0..values.len()),
    };
    typed(&values[idx], ty)
}

/// Coerce a picked string to the column's declared scalar type when it parses.
fn typed(s: &str, ty: &str) -> CborValue {
    let parsed = match ty.trim().to_lowercase().as_str() {
        "int" | "integer" | "number" | "i64" | "bigint" => {
            s.parse::<i64>().ok().map(|n| CborValue::Integer(n.into()))
        }
        "decimal" | "float" | "double" | "money" | "amount" | "f64" => {
            s.parse::<f64>().ok().map(CborValue::Float)
        }
        "bool" | "boolean" => s.parse::<bool>().ok().map(CborValue::Bool),
        _ => None,
    };
    parsed.unwrap_or_else(|| CborValue::Text(s.to_string()))
}

pub(crate) fn sentence(rng: &mut StdRng, min_words: u8, max_words: u8) -> CborValue {
    let lo = min_words.min(max_words).max(1);
    let hi = min_words.max(max_words).max(1);
    let n = rng.random_range(lo..=hi);
    let mut out = String::new();
    for i in 0..n {
        let w: String = Word().fake_with_rng(rng);
        if i == 0 {
            let mut chars = w.chars();
            if let Some(c) = chars.next() {
                out.extend(c.to_uppercase());
                out.push_str(chars.as_str());
            }
        } else {
            out.push(' ');
            out.push_str(&w);
        }
    }
    out.push('.');
    CborValue::Text(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fake::rand::SeedableRng as _;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(9)
    }

    #[test]
    fn integer_range_stays_in_bounds() {
        let mut r = rng();
        for _ in 0..500 {
            let CborValue::Integer(n) = range(&mut r, 3.0, 7.0, None) else {
                panic!("expected integer")
            };
            let n = i64::try_from(n).unwrap();
            assert!((3..=7).contains(&n));
        }
    }

    #[test]
    fn float_range_rounds_to_decimals() {
        let mut r = rng();
        for _ in 0..500 {
            let CborValue::Float(f) = range(&mut r, 0.5, 2.5, Some(2)) else {
                panic!("expected float")
            };
            assert!((0.5..=2.5).contains(&f));
            assert!(((f * 100.0).round() - f * 100.0).abs() < 1e-9, "{f}");
        }
    }

    #[test]
    fn pick_yields_listed_values_and_weights_bias() {
        let values = vec!["a".to_string(), "b".to_string()];
        let mut r = rng();
        let mut a = 0;
        for _ in 0..1000 {
            match pick(&mut r, &values, Some(&[9.0, 1.0]), "string") {
                CborValue::Text(s) if s == "a" => a += 1,
                CborValue::Text(s) if s == "b" => {}
                other => panic!("unlisted value {other:?}"),
            }
        }
        assert!(a > 800, "weights should favour `a`, got {a}/1000");
    }

    #[test]
    fn range_falls_back_instead_of_panicking_on_non_finite_bounds() {
        let mut r = rng();
        // Both bounds gone: no finite bound to fall back to.
        assert_eq!(
            range(&mut r, f64::NEG_INFINITY, f64::INFINITY, Some(2)),
            CborValue::Float(0.0)
        );
        // One finite bound survives.
        assert_eq!(
            range(&mut r, 3.0, f64::NAN, None),
            CborValue::Integer(3.into())
        );
        assert_eq!(range(&mut r, f64::NAN, 7.0, Some(1)), CborValue::Float(7.0));
    }

    #[test]
    fn pick_ignores_weights_whose_sum_overflows_to_infinity() {
        let values = vec!["a".to_string(), "b".to_string()];
        let mut r = rng();
        // Individually finite weights whose sum overflows f64::MAX; must not
        // panic, and falls back to an unweighted draw.
        for _ in 0..50 {
            match pick(&mut r, &values, Some(&[f64::MAX, f64::MAX]), "string") {
                CborValue::Text(s) => assert!(s == "a" || s == "b"),
                other => panic!("unlisted value {other:?}"),
            }
        }
    }

    #[test]
    fn pick_coerces_to_the_column_type() {
        let values = vec!["12".to_string()];
        assert_eq!(
            pick(&mut rng(), &values, None, "int"),
            CborValue::Integer(12.into())
        );
    }

    #[test]
    fn sentence_word_count_is_within_bounds() {
        let mut r = rng();
        for _ in 0..200 {
            let CborValue::Text(s) = sentence(&mut r, 3, 6) else {
                panic!("expected text")
            };
            let words = s.split_whitespace().count();
            assert!((3..=6).contains(&words), "{s}");
            assert!(s.ends_with('.'));
            assert!(s.chars().next().unwrap().is_uppercase(), "{s}");
        }
    }
}
