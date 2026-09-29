//! `row()` / `row(t)`: a generated row for a table's declared columns
//! ([`SimEngineBuilder::columns`](crate::sim::SimEngineBuilder::columns)).

use std::sync::Mutex;

use ciborium::Value as CborValue;
use vantage_rhai::rhai::{Engine, Map as RhaiMap};

use super::convert::cbor_to_dynamic;
use crate::FakerColumn;
use crate::generator::{self, Cell, Memo, column_salt, now_unix};
use crate::sim::current::{Current, VerbResult, with};
use crate::value_gen::ValueGen;

/// One column's value, drawn from the sim's own rng: `col`'s generator if
/// set, else the name/type guess [`ValueGen`] uses when seeding tables. Walk
/// and tree generators see no history across calls — `row()` has no row
/// sequence to place them on.
fn value_for(c: &mut Current, col: &FakerColumn) -> CborValue {
    match &col.generator {
        Some(generator) => {
            let memo = Mutex::new(Memo::default());
            generator::generate(
                generator,
                Cell {
                    rng: &mut c.rng,
                    memo: &memo,
                    salt: column_salt(c.id, &col.name),
                    column: &col.name,
                    ty: &col.ty,
                    seq: 0,
                    rows: generator::DEFAULT_ROWS,
                    now: now_unix(),
                },
            )
        }
        None => ValueGen::value_for_with(&mut c.rng, col),
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
    let mut map = RhaiMap::new();
    for col in &columns {
        if id_column.as_deref() == Some(col.name.as_str()) {
            continue;
        }
        let value = value_for(c, col);
        map.insert(col.name.as_str().into(), cbor_to_dynamic(&value));
    }
    Ok(map)
}

pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("row", || with(|c| row(c, None)));
    engine.register_fn("row", |t: &str| with(|c| row(c, Some(t))));
}
