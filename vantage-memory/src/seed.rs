//! Loading rows from JSON values or files, and dumping a table back out.

use std::path::Path;

use ciborium::Value as CborValue;
use vantage_core::error;
use vantage_types::Record;

use crate::{MemoryTable, Query};

/// Load `rows` into `table`. An object with an id column value upserts;
/// one without inserts, getting a generated id. Returns the number of
/// rows processed.
pub fn load(
    table: &MemoryTable,
    rows: impl IntoIterator<Item = serde_json::Value>,
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
        let record: Record<CborValue> = cbor.into();
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

/// Load rows from a `.json`, `.yaml` or `.yml` file at `path`.
pub fn load_file(table: &MemoryTable, path: &Path) -> vantage_core::Result<usize> {
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
    load(table, rows)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStore;
    use serde_json::json;

    #[test]
    fn load_upserts_by_id_and_inserts_without() {
        let t = MemoryStore::new().table("t");
        let n = load(&t, vec![json!({"id": "a", "n": 1}), json!({"n": 2})]).unwrap();
        assert_eq!(n, 2);
        assert_eq!(t.ids(), vec!["a", "1"]);
        load(&t, vec![json!({"id": "a", "n": 9})]).unwrap();
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn dump_round_trips_in_insertion_order() {
        let t = MemoryStore::new().table("t");
        let rows = vec![
            json!({"id": "b", "tags": ["x"], "m": {"k": 1.5}}),
            json!({"id": "a", "ok": true}),
        ];
        load(&t, rows.clone()).unwrap();
        assert_eq!(dump(&t).unwrap(), rows);
    }

    #[test]
    fn dump_errors_on_bytes() {
        let t = MemoryStore::new().table("t");
        let row: Record<CborValue> = [("b".to_string(), CborValue::Bytes(vec![1, 2]))]
            .into_iter()
            .collect();
        t.upsert("a", row);
        assert!(dump(&t).is_err());
    }

    #[test]
    fn non_objects_are_rejected_with_position() {
        let t = MemoryStore::new().table("t");
        let err = load(&t, vec![json!({"id": "a"}), json!(3)])
            .unwrap_err()
            .to_string();
        assert!(err.contains('1'), "{err}");
    }

    #[test]
    fn load_file_reads_yaml_and_json() {
        let dir = tempfile::tempdir().unwrap();
        let y = dir.path().join("rows.yaml");
        std::fs::write(&y, "- { id: a, n: 1 }\n- { id: b, n: 2 }\n").unwrap();
        let j = dir.path().join("rows.json");
        std::fs::write(&j, r#"[{"id": "c"}]"#).unwrap();
        let t = MemoryStore::new().table("t");
        assert_eq!(load_file(&t, &y).unwrap(), 2);
        assert_eq!(load_file(&t, &j).unwrap(), 1);
        assert_eq!(t.ids(), vec!["a", "b", "c"]);
    }
}
