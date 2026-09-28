//! Data verbs: read and write the datasource's tables.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_rhai::rhai::{Array, Dynamic, Engine, Map as RhaiMap};
use vantage_types::Record;

use crate::FakerCtx;
use crate::rhai_effect::{dynamic_to_cbor, map_to_record, record_to_map};
use crate::sim::current::{Current, VerbResult, with};

/// The store of `table`, or of the def's default table.
fn table(c: &Current, table: Option<&str>) -> VerbResult<Arc<FakerCtx>> {
    let name = table.unwrap_or(&c.kind().def.table);
    c.inner.table(name).ok_or_else(|| {
        if c.inner.tables.contains_key(name) {
            format!("table {name} was dropped").into()
        } else {
            format!("no table {name} in this datasource").into()
        }
    })
}

/// A row in declared column order, missing columns null, undeclared keys
/// after them.
fn shaped(ctx: &FakerCtx, map: &RhaiMap) -> Record<CborValue> {
    let mut rec = Record::new();
    for col in ctx.columns() {
        let value = map
            .get(col.name.as_str())
            .map_or(CborValue::Null, dynamic_to_cbor);
        rec.insert(col.name.clone(), value);
    }
    for (k, v) in map {
        if !rec.contains_key(k.as_str()) {
            rec.insert(k.to_string(), dynamic_to_cbor(v));
        }
    }
    rec
}

fn insert(c: &mut Current, t: Option<&str>, map: RhaiMap) -> VerbResult<String> {
    let ctx = table(c, t)?;
    let mut rec = shaped(&ctx, &map);
    let id_column = ctx.id_column().to_string();
    let given = map
        .get(id_column.as_str())
        .filter(|v| !v.is_unit())
        .map(|v| v.to_string())
        .filter(|s| !s.is_empty());
    Ok(match given {
        Some(id) => {
            rec.insert(id_column, CborValue::Text(id.clone()));
            ctx.put_record(&id, rec, true);
            id
        }
        None => ctx.insert_record(rec),
    })
}

fn patch(t: Option<&str>, id: &str, map: &RhaiMap) -> VerbResult<()> {
    with(|c| {
        table(c, t)?.patch_record(id, &map_to_record(map));
        Ok(())
    })
}

fn set(t: Option<&str>, id: &str, field: &str, v: &Dynamic) -> VerbResult<()> {
    with(|c| {
        table(c, t)?.update_field(id, field, dynamic_to_cbor(v));
        Ok(())
    })
}

fn delete(t: Option<&str>, id: &str) -> VerbResult<()> {
    with(|c| {
        table(c, t)?.expire(id);
        Ok(())
    })
}

fn get(c: &mut Current, t: Option<&str>, id: &str) -> VerbResult<Dynamic> {
    Ok(table(c, t)?
        .get_record(id)
        .map_or(Dynamic::UNIT, |r| Dynamic::from_map(record_to_map(&r))))
}

fn ids(c: &mut Current, t: Option<&str>) -> VerbResult<Array> {
    Ok(table(c, t)?
        .record_ids()
        .into_iter()
        .map(Dynamic::from)
        .collect())
}

pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("insert", |map: RhaiMap| with(|c| insert(c, None, map)));
    engine.register_fn("insert", |t: &str, map: RhaiMap| {
        with(|c| insert(c, Some(t), map))
    });

    engine.register_fn("patch", |id: &str, map: RhaiMap| patch(None, id, &map));
    engine.register_fn("patch", |t: &str, id: &str, map: RhaiMap| {
        patch(Some(t), id, &map)
    });

    engine.register_fn("set", |id: &str, field: &str, v: Dynamic| {
        set(None, id, field, &v)
    });
    engine.register_fn("set", |t: &str, id: &str, field: &str, v: Dynamic| {
        set(Some(t), id, field, &v)
    });

    engine.register_fn("delete", |id: &str| delete(None, id));
    engine.register_fn("delete", |t: &str, id: &str| delete(Some(t), id));

    engine.register_fn("get", |id: &str| with(|c| get(c, None, id)));
    engine.register_fn("get", |t: &str, id: &str| with(|c| get(c, Some(t), id)));

    engine.register_fn("ids", || with(|c| ids(c, None)));
    engine.register_fn("ids", |t: &str| with(|c| ids(c, Some(t))));

    engine.register_fn("count", || {
        with(|c| Ok(table(c, None)?.record_count() as i64))
    });
    engine.register_fn("count", |t: &str| {
        with(|c| Ok(table(c, Some(t))?.record_count() as i64))
    });
}
