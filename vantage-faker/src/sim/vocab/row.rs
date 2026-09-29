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
/// [`ValueGen`] uses when seeding tables. `seq` and `memo` are the calling
/// sim's running state for this table (`Current::row_calls`), so a `walk` or
/// even-spread `date` generator sees a fresh row each call instead of always
/// its row zero. A `tree` generator lays out a whole table's parent links at
/// once and is not meaningful through `row()` — it answers "which row is my
/// parent?" from a global plan `row()` never builds.
fn value_for(
    rng: &mut StdRng,
    memo: &Mutex<Memo>,
    sim_id: u64,
    seq: usize,
    col: &FakerColumn,
) -> CborValue {
    match &col.generator {
        Some(generator) => generator::generate(
            generator,
            Cell {
                rng,
                memo,
                salt: column_salt(sim_id, &col.name),
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
/// `insert`. A table with no declared columns returns an empty map.
fn row(c: &mut Current, t: Option<&str>) -> VerbResult<RhaiMap> {
    let name = t.unwrap_or(&c.kind().def.table).to_string();
    let Some(columns) = c.inner.columns.get(&name).cloned() else {
        return Ok(RhaiMap::new());
    };
    let id_column = c.inner.table(&name).map(|t| t.id_column().to_string());
    let sim_id = c.id;
    let calls = c.row_calls.entry(name).or_default();
    let seq = calls.seq;
    calls.seq += 1;
    let memo = &calls.memo;
    let rng = &mut c.rng;
    let mut map = RhaiMap::new();
    for col in &columns {
        if id_column.as_deref() == Some(col.name.as_str()) {
            continue;
        }
        let value = value_for(rng, memo, sim_id, seq, col);
        map.insert(col.name.as_str().into(), cbor_to_dynamic(&value));
    }
    Ok(map)
}

pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("row", || with(|c| row(c, None)));
    engine.register_fn("row", |t: &str| with(|c| row(c, Some(t))));
}
