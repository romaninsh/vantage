//! `row()` / `row(t)`: a generated row for a table's declared columns
//! ([`SimEngineBuilder::columns`](crate::sim::SimEngineBuilder::columns)).

use std::sync::Mutex;

use ciborium::Value as CborValue;
use fake::rand::rngs::StdRng;
use vantage_rhai::rhai::{Engine, Map as RhaiMap};

use super::convert::cbor_to_dynamic;
use crate::FakerColumn;
use crate::generator::{self, Cell, Memo, column_salt, now_unix};
use crate::sim::current::{Current, VerbResult, with};
use crate::value_gen::ValueGen;

/// One column's value: `col`'s generator if set, else the name/type guess
/// [`ValueGen`] uses when seeding tables. `seq` and `memo` both come from
/// this table's [`RowTableState`](super::super::kind::RowTableState),
/// shared by every sim spawned from the calling def; together a `walk`
/// generator sees a fresh row each call and keeps extending the same
/// series, even from a def that spawns one sim per row (`builtin:fifo`)
/// instead of looping over many. An even-spread `date` wraps: past
/// [`DEFAULT_ROWS`](generator::DEFAULT_ROWS) calls it repeats the same
/// `from..to` spread rather than pinning at `to` forever. A `tree`
/// generator lays out a whole table's parent links at once and is not
/// meaningful through `row()` — it answers "which row is my parent?" from a
/// global plan `row()` never builds.
fn value_for(
    rng: &mut StdRng,
    memo: &Mutex<Memo>,
    salt: u64,
    seq: usize,
    col: &FakerColumn,
) -> CborValue {
    match &col.generator {
        Some(generator) => generator::generate(
            generator,
            Cell {
                rng,
                memo,
                salt: column_salt(salt, &col.name),
                column: &col.name,
                ty: &col.ty,
                seq,
                rows: generator::DEFAULT_ROWS,
                now: now_unix(),
            },
        ),
        None => ValueGen::value_for_with(rng, col),
    }
}

/// Table `t`'s (or the def's default table's) declared columns, generated
/// into a map. The id column is never produced — the caller supplies it to
/// `insert` or `upsert`. A table with no declared columns returns an empty
/// map.
fn row(c: &mut Current, t: Option<&str>) -> VerbResult<RhaiMap> {
    let name = t.unwrap_or(&c.kind().def.table).to_string();
    let Some(columns) = c.inner.columns.get(&name).cloned() else {
        return Ok(RhaiMap::new());
    };
    let id_column = c.inner.table(&name).map(|t| t.id_column().to_string());
    // Salted per engine, not per sim: the series is shared, so whichever
    // sim grows it must grow the same values.
    let salt = c.inner.seed.unwrap_or_default();
    let state = c.kind().row_state(&name);
    let seq = state.next_seq();
    let rng = &mut c.rng;
    let mut map = RhaiMap::new();
    for col in &columns {
        if id_column.as_deref() == Some(col.name.as_str()) {
            continue;
        }
        let value = value_for(rng, &state.memo, salt, seq, col);
        map.insert(col.name.as_str().into(), cbor_to_dynamic(&value));
    }
    Ok(map)
}

pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("row", || with(|c| row(c, None)));
    engine.register_fn("row", |t: &str| with(|c| row(c, Some(t))));
}
