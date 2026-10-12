//! `CmdTableShell` — owns the typed `Table<Cmd, EmptyEntity>` and exposes
//! it across the `TableShell` boundary.
//!
//! `Value = CborValue` end-to-end, so `list` forwards the table's records
//! unchanged. A clone copies the table's conditions and shares the `Cmd`.
//! Equality conditions reach the script as its `conditions` scope and are
//! re-applied to rows that carry the field. Windows are cut from the listed
//! rows in memory, since a script may ignore the `limit`/`offset` it is
//! handed. Joins narrow the target through `Reference::narrow`.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Capability, Column as VistaColumn, Condition, FilterOp, Page, Preview,
    Reference as VistaReference, TableShell, Vista, VistaCapabilities, VistaMetadata, unsupported,
};

use crate::cmd::Cmd;
use crate::condition::CmdCondition;
use crate::vista::factory::CmdVistaFactory;

#[derive(Clone)]
pub struct CmdTableShell {
    table: Table<Cmd, EmptyEntity>,
    capabilities: VistaCapabilities,
    metadata: VistaMetadata,
}

impl CmdTableShell {
    pub(crate) fn new(
        table: Table<Cmd, EmptyEntity>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
        }
    }

    /// Everything that decides the argv: the locked program, the script, and
    /// the values seeded into its scope.
    fn preview_scope(&self) -> serde_json::Value {
        let script_name = self.table.table_name();
        let source = self.table.data_source();
        let conditions: Vec<serde_json::Value> = self
            .table
            .conditions()
            .map(|c| {
                serde_json::json!({
                    "field": c.field(),
                    "op": c.op(),
                    "value": c.json_value(),
                })
            })
            .collect();
        serde_json::json!({
            "command": source.command(),
            "script_name": script_name,
            "script": source
                .spec_for(script_name)
                .map(|spec| spec.script.to_string())
                .unwrap_or_else(|e| format!("<no script registered: {e}>")),
            "scope": {
                "columns": self.table.columns().keys().collect::<Vec<_>>(),
                "conditions": conditions,
            },
        })
    }
}

#[async_trait]
impl TableShell for CmdTableShell {
    fn columns(&self) -> &IndexMap<String, VistaColumn> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, VistaReference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "cmd"
    }

    /// A cmd table's argv is built *by* its Rhai script, which calls `run(...)`
    /// itself, so there is no argv to render without running the command. The
    /// preview reports what decides it instead.
    fn preview(&self) -> Preview {
        Preview::new("cmd")
            .with_query(self.preview_scope().to_string())
            .with_note(
                "the script builds its own argv and runs it; these are the scope \
                 values it would be evaluated with",
            )
    }

    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        self.table.list_values().await
    }

    /// The command's DETAIL script when the table declares one (a direct
    /// lookup by id); otherwise the set is listed and the row picked out.
    /// The detail script doesn't see the conditions, so on a narrowed set
    /// the id must first be listed in the set.
    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        let has_detail = self
            .table
            .data_source()
            .has_detail_script(self.table.table_name());
        if self.table.conditions().next().is_none() || !has_detail {
            return self.table.get_value(id).await;
        }
        if !self.table.list_values().await?.contains_key(id) {
            return Ok(None);
        }
        self.table.get_value(id).await
    }

    async fn count(&self) -> Result<i64> {
        Ok(self.table.list_values().await?.len() as i64)
    }

    /// The window is cut from the listed rows, which are all read anyway, so
    /// the page carries the set's total for free.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let rows = self.table.list_values().await?;
        let total = rows.len() as i64;
        let window = rows.into_iter().skip(offset).take(limit).collect();
        Ok(Page::new(window).with_total(total))
    }

    /// Equality on a declared column. The script receives it in its
    /// `conditions` scope; no other operator is passed on.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        if !self.metadata.columns.contains_key(field) {
            return Err(error!("Unknown column for condition", field = field));
        }
        if condition.op != FilterOp::Eq {
            return unsupported(Capability::FilterOps);
        }
        self.table
            .add_condition(CmdCondition::eq(field, condition.value.clone()));
        Ok(())
    }

    /// The target comes from the table's `with_one`/`with_many`; from a row
    /// it is narrowed by the reference's join keys.
    fn get_ref(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        let reference = self
            .metadata
            .references
            .get(relation)
            .ok_or_else(|| error!("Unknown relation", relation = relation).mark_not_found())?;
        let target = self.table.get_ref_target_erased(relation)?;
        let target = CmdVistaFactory::new(self.table.data_source().clone()).from_table(target)?;
        match row {
            Some(row) => reference.narrow(target, row, CborValue::clone),
            None => Ok(target),
        }
    }
}
