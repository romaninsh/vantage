//! `CmdTableShell` — owns the typed `Table<Cmd, EmptyEntity>` and exposes
//! it across the `TableShell` boundary.
//!
//! `Value = CborValue` end-to-end, so `list_vista_values` forwards the
//! table's records unchanged. Relations are followed the built-in way:
//! `get_ref` forwards to the wrapped table's [`Table::get_ref_from_row`],
//! which reads the join value out of the parent row and pins the target
//! table with a plain eq-condition — the same path csv / sqlite use.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Column as VistaColumn, Reference as VistaReference, TableShell, Vista, VistaCapabilities,
    VistaMetadata,
};

use crate::cmd::Cmd;
use crate::condition::CmdCondition;
use crate::vista::factory::CmdVistaFactory;

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

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.table.list_values().await
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        // Route through the typed table so detail-script tables hydrate via
        // the DETAIL script (Cmd::get_table_value), not the list script.
        self.table.get_value(id).await
    }

    async fn get_vista_value_with_row(
        &self,
        _vista: &Vista,
        id: &String,
        row: &Record<CborValue>,
    ) -> Result<Option<Record<CborValue>>> {
        // The detail pass hands us the cheap list-pass row so the detail script
        // can read columns it carries (e.g. step numbers painted from the
        // reference conditions).
        self.table
            .data_source()
            .get_table_value_with_row(&self.table, id, row)
            .await
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let data = self.table.list_values().await?;
        Ok(data.into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        Ok(self.table.list_values().await?.len() as i64)
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        self.table
            .add_condition(CmdCondition::eq(field, value.clone()));
        Ok(())
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        // `Cmd::Value` is `CborValue`, so the row passes through unchanged.
        // The table's reference machinery reads the join value out of the
        // parent row and pins the target with a plain eq-condition.
        let target = self.table.get_ref_from_row::<EmptyEntity>(relation, row)?;
        CmdVistaFactory::new(self.table.data_source().clone()).from_table(target)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        let target = self.table.get_ref_target_erased(relation)?;
        CmdVistaFactory::new(self.table.data_source().clone()).from_table(target)
    }

    fn get_ref_kinds(&self) -> Vec<(String, vantage_vista::ReferenceKind)> {
        self.table.ref_kinds()
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "cmd"
    }

    /// A cmd table's argv is built *by* its Rhai script, and that script builds
    /// it by calling `run(...)` — so there is no argv to render without running
    /// the command. What this reports instead is everything that decides the
    /// argv: the locked program, the script itself, and the values seeded into
    /// its scope. Read the script against these and you can see the command it
    /// will assemble.
    fn preview_query(&self, _vista: &Vista) -> serde_json::Value {
        let script_name = self.table.table_name();
        let source = self.table.data_source();
        let (limit, offset) = match self.table.pagination() {
            Some(p) => (Some(p.limit()), Some(p.skip())),
            None => (None, None),
        };
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
            "driver": "cmd",
            "command": source.command(),
            "script_name": script_name,
            "script": source
                .spec_for(script_name)
                .map(|spec| spec.script.to_string())
                .unwrap_or_else(|e| format!("<no script registered: {e}>")),
            "scope": {
                "columns": self.table.columns().keys().collect::<Vec<_>>(),
                "conditions": conditions,
                "limit": limit,
                "offset": offset,
            },
            "note": "the script builds its own argv and runs it; these are the \
                     scope values it would be evaluated with",
        })
    }
}
