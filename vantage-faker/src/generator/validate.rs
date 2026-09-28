//! Load-time checks for [`ColumnGen`]. Generation itself never fails — a bad
//! generator emits null or falls back to a safe value — so a config loader
//! calls [`ColumnGen::validate`] to report the mistake instead.

use super::{ColumnGen, date};

impl ColumnGen {
    /// Check the parameters a generator cannot use as written: an empty pick
    /// list or mismatched/degenerate weights, non-finite or inverted bounds
    /// (range, walk, sentence, date), an unparsable date, and a tree with no
    /// roots or no levels.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Pick { values, weights } => {
                if values.is_empty() {
                    return Err("pick: `values` is empty".into());
                }
                if let Some(w) = weights {
                    if w.len() != values.len() {
                        return Err(format!(
                            "pick: {} weights for {} values",
                            w.len(),
                            values.len()
                        ));
                    }
                    if w.iter().any(|x| !x.is_finite() || *x < 0.0) {
                        return Err("pick: weights must be finite and non-negative".into());
                    }
                    let sum = w.iter().sum::<f64>();
                    if !sum.is_finite() || sum <= 0.0 {
                        return Err("pick: weights must have a finite, positive sum".into());
                    }
                }
                Ok(())
            }
            Self::Range { min, max, .. } if !min.is_finite() || !max.is_finite() => Err(format!(
                "range: min {min} and max {max} must both be finite"
            )),
            Self::Range { min, max, .. } if min > max => {
                Err(format!("range: min {min} is above max {max}"))
            }
            Self::Walk {
                start,
                step,
                min,
                max,
                ..
            } if !start.is_finite()
                || !step.is_finite()
                || min.is_some_and(|lo| !lo.is_finite())
                || max.is_some_and(|hi| !hi.is_finite()) =>
            {
                Err(format!(
                    "walk: start {start}, step {step}, min {min:?} and max {max:?} must all be finite"
                ))
            }
            Self::Date { from, to, .. } => {
                // Relative bounds shift together, so any shared `now` will do.
                let resolve = |end: &str| {
                    date::parse_when(end, date::now_unix())
                        .ok_or_else(|| format!("date: cannot parse `{end}`"))
                };
                let (a, b) = (resolve(from)?, resolve(to)?);
                if a > b {
                    return Err(format!("date: from `{from}` is later than to `{to}`"));
                }
                Ok(())
            }
            Self::Sentence {
                min_words,
                max_words,
            } if min_words > max_words => Err(format!(
                "sentence: min_words {min_words} is above max_words {max_words}"
            )),
            Self::Walk {
                min: Some(lo),
                max: Some(hi),
                ..
            } if lo > hi => Err(format!("walk: min {lo} is above max {hi}")),
            Self::Tree { roots: 0, .. } => Err("tree: `roots` must be at least 1".into()),
            Self::Tree { depth: 0, .. } => {
                Err("tree: `depth` must be at least 1 (1 = every row is a root)".into())
            }
            _ => Ok(()),
        }
    }
}
