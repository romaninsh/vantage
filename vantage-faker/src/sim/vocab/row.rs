//! `fake_row()`: a generated row for a table's declared columns, called on
//! a [`Handle`] from `table()` or `table(name)`.

use std::sync::Mutex;

use ciborium::Value as CborValue;
use fake::rand::rngs::StdRng;
use vantage_rhai::rhai::{Engine, Map as RhaiMap};
use vantage_vista::Handle;

use crate::FakerColumn;
use crate::generator::{self, Cell, Memo, column_salt, now_unix};
use crate::sim::current::{Current, VerbResult, with};
use crate::value_gen::ValueGen;
use vantage_vista::rhai::cbor_to_dynamic;

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
        None => ValueGen::value_for_with(rng, col, now_unix()),
    }
}

/// Table `name`'s declared columns, generated into a map. The id column is
/// never produced — the caller supplies it to `insert` or `upsert`. A table
/// with no declared columns returns an empty map.
fn row(c: &mut Current, name: &str) -> VerbResult<RhaiMap> {
    let Some(columns) = c.inner.columns.get(name).cloned() else {
        return Ok(RhaiMap::new());
    };
    let id_column = c.inner.table(name).map(|t| t.id_column().to_string());
    // Salted per engine, not per sim: the series is shared, so whichever
    // sim grows it must grow the same values.
    let salt = c.inner.seed.unwrap_or_default();
    let state = c.kind().row_state(name);
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
    engine.register_fn("fake_row", |h: &mut Handle| -> VerbResult<RhaiMap> {
        let name = h
            .table_name()
            .ok_or("fake_row only works on a handle from table(...)")?
            .to_string();
        with(|c| row(c, &name))
    });
}
