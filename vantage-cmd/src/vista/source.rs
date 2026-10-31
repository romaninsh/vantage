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
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Capability, Condition, FilterOp, Page, Preview, TableShell, Transaction, Vista,
    VistaCapabilities, VistaMetadata, condition_column_error, direct, unknown_relation,
    unsupported,
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
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "cmd"
    }

    /// A cmd table's argv is built *by* its Rhai script, which calls `run(...)`
    /// itself, so there is no argv to render without running the command. The
    /// query is what decides it instead: the command, the script, and the
    /// scope it is evaluated with, every condition included.
    fn preview(&self) -> Preview {
        Preview::new().with_query(self.preview_scope().to_string())
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
        Ok(Page::from_listing(
            self.table.list_values().await?,
            offset,
            limit,
        ))
    }

    /// Equality on a declared column. The script receives it in its
    /// `conditions` scope; no other operator is passed on. A dotted path
    /// into a declared column is refused, for the caller to evaluate.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        if !self.metadata.columns.contains_key(field) {
            return Err(condition_column_error(&self.metadata, field));
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
            .ok_or_else(|| unknown_relation(relation))?;
        self.table.traverse_erased(reference, row, |target| {
            CmdVistaFactory::new(self.table.data_source().clone()).from_table(target)
        })
    }

    /// A command has no transactions: inside one, the shell works as it
    /// does outside.
    fn rebind(&self, _tx: &Transaction) -> Result<Box<dyn TableShell>> {
        Ok(direct(Box::new(self.clone())))
    }
}
