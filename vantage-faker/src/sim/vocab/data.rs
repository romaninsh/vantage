//! Data verbs: read and write the store's tables.

use vantage_memory::{MemoryCondition, MemoryTableHandle, Query, UpsertOutcome};
use vantage_rhai::rhai::{Array, Dynamic, Engine, Map as RhaiMap};
use vantage_vista::FilterOp;

use super::convert::{dynamic_to_cbor, map_to_record, record_to_map};
use crate::sim::current::{Current, VerbResult, with};
use crate::sim::stats::Counters;

/// Table `t` of the store, or the def's default table.
fn table(c: &Current, t: Option<&str>) -> VerbResult<MemoryTableHandle> {
    let name = t.unwrap_or(&c.kind().def.table);
    c.inner
        .table(name)
        .ok_or_else(|| format!("no table {name} in this store").into())
}

/// Bump the write counter of the sim's engine.
fn wrote(c: &Current) {
    Counters::bump(&c.inner.counters.writes);
}

/// Run `write` on table `t`, counting a write when it reports a change.
fn write(t: Option<&str>, write: impl FnOnce(&MemoryTableHandle) -> bool) -> VerbResult<()> {
    with(|c| {
        if write(&table(c, t)?) {
            wrote(c);
        }
        Ok(())
    })
}

fn insert(c: &mut Current, t: Option<&str>, map: RhaiMap) -> VerbResult<String> {
    let table = table(c, t)?;
    let given = map
        .get(table.id_column())
        .filter(|v| !v.is_unit())
        .map(|v| v.to_string())
        .filter(|s| !s.is_empty());
    let rec = map_to_record(&map);
    let id = match given {
        Some(id) => table.insert_as(&id, rec).map(|_| id),
        None => table.insert(rec),
    }
    .map_err(|e| e.to_string())?;
    wrote(c);
    Ok(id)
}

fn upsert(t: Option<&str>, id: &str, map: &RhaiMap) -> VerbResult<()> {
    write(t, |table| {
        table.upsert(id, map_to_record(map)) != UpsertOutcome::Unchanged
    })
}

fn patch(t: Option<&str>, id: &str, map: &RhaiMap) -> VerbResult<()> {
    write(t, |table| table.patch(id, &map_to_record(map)))
}

fn set(t: Option<&str>, id: &str, field: &str, v: &Dynamic) -> VerbResult<()> {
    let mut partial = vantage_types::Record::new();
    partial.insert(field.to_string(), dynamic_to_cbor(v));
    write(t, |table| table.patch(id, &partial))
}

fn delete(t: Option<&str>, id: &str) -> VerbResult<()> {
    write(t, |table| table.delete(id))
}

fn get(c: &mut Current, t: Option<&str>, id: &str) -> VerbResult<Dynamic> {
    Ok(table(c, t)?
        .get(id)
        .map_or(Dynamic::UNIT, |r| Dynamic::from_map(record_to_map(&r))))
}

fn ids(c: &mut Current, t: Option<&str>) -> VerbResult<Array> {
    Ok(table(c, t)?.ids().into_iter().map(Dynamic::from).collect())
}

/// Ids of the rows equal to every entry of `map`, in insertion order.
fn find(c: &mut Current, t: Option<&str>, map: &RhaiMap) -> VerbResult<Array> {
    let q = map.iter().fold(Query::new(), |q, (k, v)| {
        q.filter(MemoryCondition::cmp(
            k.as_str(),
            FilterOp::Eq,
            dynamic_to_cbor(v),
        ))
    });
    let rows = table(c, t)?.query(&q).map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|(id, _)| Dynamic::from(id)).collect())
}

pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("insert", |map: RhaiMap| with(|c| insert(c, None, map)));
    engine.register_fn("insert", |t: &str, map: RhaiMap| {
        with(|c| insert(c, Some(t), map))
    });

    engine.register_fn("upsert", |id: &str, map: RhaiMap| upsert(None, id, &map));
    engine.register_fn("upsert", |t: &str, id: &str, map: RhaiMap| {
        upsert(Some(t), id, &map)
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

    engine.register_fn("find", |map: RhaiMap| with(|c| find(c, None, &map)));
    engine.register_fn("find", |t: &str, map: RhaiMap| {
        with(|c| find(c, Some(t), &map))
    });

    engine.register_fn("count", || with(|c| Ok(table(c, None)?.len() as i64)));
    engine.register_fn("count", |t: &str| {
        with(|c| Ok(table(c, Some(t))?.len() as i64))
    });
}
