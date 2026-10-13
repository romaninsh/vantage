//! The list read: conditions resolved, the script run on a blocking thread,
//! its rows keyed and re-filtered.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_table::table::{Table, retain_matching};
use vantage_types::{Entity, Record};

use crate::cmd::Cmd;
use crate::condition::CmdCondition;
use crate::rhai_engine::QueryContext;
use crate::types::{cbor_to_string, json_to_cbor};

/// Convert script-produced JSON rows into id-keyed records. The id comes
/// from the `id_field` value on each row, falling back to the row index.
pub(super) fn rows_to_records(
    rows: Vec<JsonValue>,
    id_field: Option<&str>,
) -> IndexMap<String, Record<CborValue>> {
    let mut records: IndexMap<String, Record<CborValue>> = IndexMap::new();
    for (idx, row) in rows.into_iter().enumerate() {
        let mut record = Record::new();
        match row {
            JsonValue::Object(map) => {
                for (k, v) in map {
                    record.insert(k, json_to_cbor(&v));
                }
            }
            other => {
                record.insert("value".to_string(), json_to_cbor(&other));
            }
        }
        let id = id_field
            .and_then(|f| record.get(f))
            .map(cbor_to_string)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| idx.to_string());
        records.insert(id, record);
    }
    records
}

/// Keep the rows matching every `Eq` and `In` condition, through
/// vantage-table's [`retain_matching`]: a row lacking a row field doesn't
/// match; one lacking a request flag is left alone.
fn retain_conditions(
    records: &mut IndexMap<String, Record<CborValue>>,
    conditions: &[CmdCondition],
    row_fields: &[String],
) {
    let eqs: Vec<(&str, &CborValue)> = conditions
        .iter()
        .filter_map(|c| match c {
            CmdCondition::Eq { field, value } => Some((field.as_str(), value)),
            _ => None,
        })
        .collect();
    retain_matching(records, &eqs, row_fields, |held, wanted| held == wanted);

    let ins: Vec<(&str, &Vec<CborValue>)> = conditions
        .iter()
        .filter_map(|c| match c {
            CmdCondition::In { field, values } => Some((field.as_str(), values)),
            _ => None,
        })
        .collect();
    retain_matching(records, &ins, row_fields, |held, wanted| {
        wanted.contains(held)
    });
}

impl Cmd {
    /// Resolve Deferred (relation) conditions to concrete `Eq`/`In`, so the
    /// command never sees a subquery.
    async fn resolved_conditions<E: Entity<CborValue>>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Vec<CmdCondition>> {
        let mut conditions = Vec::new();
        for cond in table.conditions().cloned() {
            let CmdCondition::Deferred { field, source } = cond else {
                conditions.push(cond);
                continue;
            };
            conditions.push(match ExprDataSource::execute(self, &source).await? {
                CborValue::Array(values) => CmdCondition::In { field, values },
                value => CmdCondition::Eq { field, value },
            });
        }
        Ok(conditions)
    }

    /// The rows the table's list script returns under its conditions and
    /// pagination. Conditions naming row fields are re-applied to the rows,
    /// as a safety net for a script that ignores them.
    pub(super) async fn list_rows<E: Entity<CborValue>>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let table_name = table.table_name().to_string();
        let id_field = table.id_field().map(|c| c.name().to_string());
        let conditions = self.resolved_conditions(table).await?;
        let (limit, offset) = match table.pagination() {
            Some(p) => (Some(p.limit()), Some(p.skip())),
            None => (None, None),
        };

        let ctx = QueryContext {
            conditions: conditions.clone(),
            columns: table.columns().keys().cloned().collect(),
            limit,
            offset,
            id_column: id_field.clone(),
            id: None,
            row: CborValue::Map(vec![]),
        };

        let cmd = self.clone();
        let rows: Vec<JsonValue> = tokio::task::spawn_blocking(move || {
            let compiled = cmd.compiled_list_script(&table_name)?;
            compiled.eval(ctx)
        })
        .await
        .map_err(|e| error!("command task failed to join", detail = e.to_string()))??;

        let mut records = rows_to_records(rows, id_field.as_deref());
        retain_conditions(&mut records, &conditions, &table.row_fields());
        Ok(records)
    }
}
