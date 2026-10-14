//! Loading rows from JSON values or files, and dumping a table back out.

use std::path::Path;

use ciborium::Value as CborValue;
use vantage_core::error;
use vantage_types::Record;

use crate::{MemoryTable, Query};

/// Load `rows` into `table`. An object with an id column value upserts;
/// one without inserts, getting a generated id. Returns the number of
/// rows processed. Rows are stored as given; the Vista factory conforms
/// them to their declared column types.
pub fn load(
    table: &MemoryTable,
    rows: impl IntoIterator<Item = serde_json::Value>,
) -> vantage_core::Result<usize> {
    load_with(table, rows, |_| {})
}

/// [`load`], passing each row through `prepare` before it is stored (the
/// Vista factory conforms cells to their declared column types here).
pub(crate) fn load_with(
    table: &MemoryTable,
    rows: impl IntoIterator<Item = serde_json::Value>,
    mut prepare: impl FnMut(&mut Record<CborValue>),
) -> vantage_core::Result<usize> {
    let mut n = 0;
    for (i, value) in rows.into_iter().enumerate() {
        let cbor = CborValue::serialized(&value).map_err(|e| {
            error!(
                "Cannot convert seed row to CBOR",
                position = i,
                detail = e.to_string()
            )
        })?;
        let CborValue::Map(_) = &cbor else {
            return Err(error!("Seed row is not an object", position = i));
        };
        let mut record: Record<CborValue> = cbor.into();
        prepare(&mut record);
        match crate::store::ids::supplied_id(record.get(table.id_column())) {
            Some(id) => {
                table.upsert(&id, record);
            }
            None => {
                table.insert(record)?;
            }
        }
        n += 1;
    }
    Ok(n)
}

/// Load rows from a `.json`, `.yaml` or `.yml` file at `path`. Rows are
/// stored as given.
pub fn load_file(table: &MemoryTable, path: &Path) -> vantage_core::Result<usize> {
    load_file_with(table, path, |_| {})
}

/// [`load_file`], passing each row through `prepare` as [`load_with`] does.
pub(crate) fn load_file_with(
    table: &MemoryTable,
    path: &Path,
    prepare: impl FnMut(&mut Record<CborValue>),
) -> vantage_core::Result<usize> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        error!(
            "Cannot read seed file",
            path = path.display(),
            detail = e.to_string()
        )
    })?;
    let rows: Vec<serde_json::Value> = match path.extension().and_then(|e| e.to_str()) {
        Some("yaml") | Some("yml") => serde_yaml_ng::from_str(&content).map_err(|e| {
            error!(
                "Cannot read seed file",
                path = path.display(),
                detail = e.to_string()
            )
        })?,
        _ => serde_json::from_str(&content).map_err(|e| {
            error!(
                "Cannot read seed file",
                path = path.display(),
                detail = e.to_string()
            )
        })?,
    };
    load_with(table, rows, prepare)
}

/// Every row in `table`, as JSON objects in insertion order. A row holding
/// a cell JSON cannot express (bytes, tags, non-text map keys) is an error.
pub fn dump(table: &MemoryTable) -> vantage_core::Result<Vec<serde_json::Value>> {
    table
        .query(&Query::new())?
        .into_iter()
        .map(|(id, row)| {
            let value: CborValue = (*row).clone().into();
            value.deserialized().map_err(|e| {
                error!(
                    "Cannot dump row as JSON",
                    table = table.name(),
                    id = id,
                    detail = e.to_string()
                )
            })
        })
        .collect()
}
