//! Explicit column generators — [`ColumnGen`] overrides the name/type guess
//! of [`ValueGen`](crate::ValueGen) for one column.
//!
//! Generators split into two families:
//! - *stream* generators ([`Pick`](ColumnGen::Pick), [`Range`](ColumnGen::Range),
//!   random [`Date`](ColumnGen::Date), [`Sentence`](ColumnGen::Sentence),
//!   [`Pattern`](ColumnGen::Pattern)) draw from the generator's rng, like the
//!   name/type fallback does;
//! - *positional* generators ([`Walk`](ColumnGen::Walk),
//!   [`Tree`](ColumnGen::Tree), even-spread [`Date`](ColumnGen::Date)) are a
//!   function of the seed, the column name and the row's `seq` alone, so a
//!   row's value does not depend on how many rows were generated before it.

mod date;
pub(crate) mod hash;
mod pattern;
mod scalar;
#[cfg(test)]
mod tests;
mod tree;
mod validate;
mod walk;
mod wire;

use std::collections::HashMap;
use std::sync::Mutex;

use ciborium::Value as CborValue;
use fake::rand::rngs::StdRng;
use serde::Deserialize;

pub(crate) use date::now_unix;

/// Table size that even-spread dates and trees assume when the generator was
/// not told one (see [`ValueGen::with_rows`](crate::ValueGen::with_rows)).
pub(crate) const DEFAULT_ROWS: usize = 100;
pub(crate) use hash::column_salt;

/// How a column's value is generated when name/type matching is not enough.
///
/// Deserializes from a map with a single key naming the generator (snake_case
/// variant name), e.g. `{ range: { min: 1, max: 5 } }` or
/// `{ pattern: "BA####" }`. Optional fields default as documented per variant.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "wire::Wire")]
pub enum ColumnGen {
    /// One of `values`, uniformly or by `weights` (same length as `values`;
    /// otherwise ignored). Emitted as text, or as an integer / float / bool
    /// when the column's declared type is one and the value parses as it.
    Pick {
        values: Vec<String>,
        weights: Option<Vec<f64>>,
    },
    /// A number in `[min, max]`: an integer when `decimals` is absent or 0,
    /// otherwise a float rounded to `decimals` places.
    Range {
        min: f64,
        max: f64,
        decimals: Option<u8>,
    },
    /// An RFC 3339 UTC timestamp (`2026-09-28T14:05:00Z`) between `from` and
    /// `to` (default `now`). Each end is `now`, a signed offset from now
    /// (`-90d`, `+2w`, `-12h`, `-30m`, `-45s`) or an ISO date / date-time.
    /// `spread` defaults to [`Spread::Random`].
    Date {
        from: String,
        to: String,
        spread: Spread,
    },
    /// Lorem words, capitalised, ending with a full stop. Word count is drawn
    /// from `[min_words, max_words]`, default `[4, 10]`.
    Sentence { min_words: u8, max_words: u8 },
    /// A code shaped like the template: `#` digit, `?` uppercase letter,
    /// `*` uppercase letter or digit, `\` escapes the next character, anything
    /// else is literal — `BA####`, `Gate ?##`, `G-****`.
    Pattern(String),
    /// A clamped random walk over `seq`: row 0 is `start`, each next row moves
    /// by up to `±step`. Output follows [`Range`](Self::Range)'s `decimals`
    /// rule.
    Walk {
        start: f64,
        step: f64,
        min: Option<f64>,
        max: Option<f64>,
        decimals: Option<u8>,
    },
    /// Same-table parent link: rows `0..roots` get a null parent, every other
    /// row a [`seed_id`](crate::seed_id) of a row with a smaller `seq`.
    /// `depth` counts levels including the roots, so `depth: 1` makes every
    /// row a root. Rows are laid out breadth-first with an even fan-out.
    Tree { roots: usize, depth: u8 },
}

/// How [`ColumnGen::Date`] places rows between `from` and `to`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spread {
    /// Independent uniform draw per row.
    #[default]
    Random,
    /// Row `seq` of `n` sits at step `seq` of `n - 1` from `from` to `to`,
    /// with a jitter of under half a step — a time series ordered by id.
    Even,
}

/// Per-[`ValueGen`](crate::ValueGen) cache for positional generators, shared
/// across clones: walk series and tree plans computed once per table.
#[derive(Default)]
pub(crate) struct Memo {
    walks: HashMap<String, Vec<f64>>,
    trees: HashMap<String, tree::Plan>,
}

/// Everything a generator may read for one cell.
pub(crate) struct Cell<'a> {
    pub rng: &'a mut StdRng,
    pub memo: &'a Mutex<Memo>,
    /// Seed (or per-generator entropy) mixed with the column name.
    pub salt: u64,
    pub column: &'a str,
    pub ty: &'a str,
    pub seq: usize,
    /// Rows in the table being built, [`DEFAULT_ROWS`] when the caller did
    /// not say.
    pub rows: usize,
    /// Unix seconds that `now` resolves to.
    pub now: i64,
}

/// Generate one value for `generator` at `cell`.
pub(crate) fn generate(generator: &ColumnGen, mut cell: Cell<'_>) -> CborValue {
    match generator {
        ColumnGen::Pick { values, weights } => {
            scalar::pick(cell.rng, values, weights.as_deref(), cell.ty)
        }
        ColumnGen::Range { min, max, decimals } => scalar::range(cell.rng, *min, *max, *decimals),
        ColumnGen::Date { from, to, spread } => date::generate(&mut cell, from, to, *spread),
        ColumnGen::Sentence {
            min_words,
            max_words,
        } => scalar::sentence(cell.rng, *min_words, *max_words),
        ColumnGen::Pattern(template) => CborValue::Text(pattern::expand(cell.rng, template)),
        ColumnGen::Walk {
            start,
            step,
            min,
            max,
            decimals,
        } => {
            let params = walk::Params {
                start: *start,
                step: *step,
                min: *min,
                max: *max,
            };
            let key = format!("{}|{params:?}", cell.column);
            let mut memo = cell.memo.lock().unwrap();
            let series = memo.walks.entry(key).or_default();
            let v = walk::value_at(series, &params, cell.salt, cell.seq);
            scalar::number(v, *decimals)
        }
        ColumnGen::Tree { roots, depth } => {
            let count = cell.rows;
            let key = format!("{}|{roots}|{depth}|{count}", cell.column);
            let mut memo = cell.memo.lock().unwrap();
            let plan = memo
                .trees
                .entry(key)
                .or_insert_with(|| tree::Plan::new(*roots, *depth, count, cell.salt));
            match plan.parent_of(cell.seq, cell.salt) {
                Some(parent) => CborValue::Text(crate::seed_id(parent)),
                None => CborValue::Null,
            }
        }
    }
}
