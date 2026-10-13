//! The detail read: one id hydrated by the table's DETAIL script.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_table::table::Table;
use vantage_types::{Entity, Record};

use super::list::rows_to_records;
use crate::cmd::Cmd;
use crate::rhai_engine::QueryContext;
use crate::types::cbor_to_string;

/// The DETAIL script's answer for `id`: the row keyed by `id`; else, when
/// the script returned exactly one row and that row carries no id, that row
/// stamped with `id` (detail scripts commonly return just the extra fields).
/// A row carrying another id, or several id-less rows, answer `None`.
fn detail_row(
    mut records: IndexMap<String, Record<CborValue>>,
    id_field: Option<&str>,
    id: &str,
) -> Option<Record<CborValue>> {
    if let Some(row) = records.shift_remove(id) {
        return Some(row);
    }
    if records.len() != 1 {
        return None;
    }
    let (_, mut row) = records.pop()?;
    let carried = id_field
        .and_then(|f| row.get(f))
        .is_some_and(|v| !cbor_to_string(v).is_empty());
    if carried {
        return None;
    }
    if let Some(field) = id_field {
        row.insert(field.to_string(), CborValue::Text(id.to_string()));
    }
    Some(row)
}

impl Cmd {
    /// Detail-script hydration for one id, with the existing list-pass `row`
    /// injected into the script scope as `row`. Falls back to the normal
    /// list-and-pick path when the table has no detail script. The detail
    /// script runs without the table's conditions. Its answer is the row
    /// keyed by `id`, or a single row carrying no id, stamped with `id`.
    pub async fn get_table_value_with_row<E>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        row: &Record<CborValue>,
    ) -> Result<Option<Record<CborValue>>>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        let table_name = table.table_name().to_string();
        if !self.has_detail_script(&table_name) {
            let mut all = self.list_rows(table).await?;
            return Ok(all.shift_remove(id));
        }

        let id_field = table.id_field().map(|c| c.name().to_string());
        let columns: Vec<String> = table.columns().keys().cloned().collect();
        let row_map = CborValue::Map(
            row.iter()
                .map(|(k, v)| (CborValue::Text(k.clone()), v.clone()))
                .collect(),
        );
        let ctx = QueryContext {
            conditions: Vec::new(),
            columns,
            limit: None,
            offset: None,
            id_column: id_field.clone(),
            id: Some(id.to_string()),
            row: row_map,
        };
        let cmd = self.clone();
        let rows: Vec<JsonValue> = tokio::task::spawn_blocking(move || {
            let compiled = cmd
                .compiled_detail_script(&table_name)?
                .ok_or_else(|| error!("detail script vanished"))?;
            compiled.eval(ctx)
        })
        .await
        .map_err(|e| error!("command task failed to join", detail = e.to_string()))??;

        Ok(detail_row(
            rows_to_records(rows, id_field.as_deref()),
            id_field.as_deref(),
            id,
        ))
    }
}
