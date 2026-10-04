//! `fake_row()`: a generated row for a table's declared columns, called on
//! a [`Handle`] from `table()` or `table(name)`.

use vantage_rhai::rhai::{Engine, Map as RhaiMap};
use vantage_vista::Handle;

use crate::generator::now_unix;
use crate::sim::current::{Current, VerbResult, with};
use crate::value_gen::{Draw, ValueGen};
use vantage_vista::rhai::cbor_to_dynamic;

/// Table `name`'s declared columns, generated into a map by
/// [`ValueGen::cell`], as seeding the table does. The id column is never
/// produced — the caller supplies it to `insert` or `upsert`. A table with
/// no declared columns returns an empty map.
///
/// `seq` and the memo come from this table's
/// [`RowTableState`](super::super::kind::RowTableState), shared by every sim
/// spawned from the calling def; together a `walk` generator sees a fresh
/// row each call and keeps extending the same series, even from a def that
/// spawns one sim per row (`builtin:fifo`) instead of looping over many. An
/// even-spread `date` wraps: past the table's row count it repeats the same
/// `from..to` spread rather than pinning at `to` forever. A `tree`
/// generator lays out a whole table's parent links at once and is not
/// meaningful through `fake_row()` — it answers "which row is my parent?"
/// from a global plan `fake_row()` never builds.
fn row(c: &mut Current, name: &str) -> VerbResult<RhaiMap> {
    let Some(columns) = c.inner.columns.get(name).cloned() else {
        return Ok(RhaiMap::new());
    };
    let id_column = c.inner.table(name).map(|t| t.id_column().to_string());
    let settings = c.inner.fake_rows.get(name).copied().unwrap_or_default();
    let state = c.kind().row_state(name);
    let mut draw = Draw {
        rng: &mut c.rng,
        memo: &state.memo,
        // Salted per engine, not per sim: the series is shared, so whichever
        // sim grows it must grow the same values.
        salt: c.inner.seed.unwrap_or_default(),
        seq: state.next_seq(),
        rows: settings.rows,
        now: now_unix(),
        weirdness: settings.weirdness,
    };
    let mut map = RhaiMap::new();
    for col in &columns {
        if id_column.as_deref() == Some(col.name.as_str()) {
            continue;
        }
        let value = ValueGen::cell(&mut draw, col);
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
