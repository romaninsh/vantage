//! `GraphqlApiTableShell` — wraps an [`AnyTable`] for the Vista universal
//! surface.
//!
//! Why an `AnyTable` instead of the typed `Table<GraphqlApi, E>` like the
//! REST shell does? GraphQL speaks `AnyGraphqlType` (wrapping
//! `serde_json::Value`) natively, but Vista's contract is
//! `TableLike<Value = CborValue, Id = String>`. The `CborAdapter` blanket
//! inside `AnyTable` provides that bridge for free — we'd otherwise have
//! to re-implement the conversion path here.
//!
//! The shell forwards every Vista call through to the wrapped `AnyTable`;
//! the only GraphQL-specific bit is the driver name that surfaces in
//! Vista metadata.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::any::AnyTable;
use vantage_table::traits::table_like::TableLike;
use vantage_types::Record;
use vantage_vista::{TableShell, Vista, VistaCapabilities};

use crate::rest::vista::AnyTableShell;

pub struct GraphqlApiTableShell {
    pub(crate) table: AnyTable,
    pub(crate) capabilities: VistaCapabilities,
}

impl GraphqlApiTableShell {
    pub(crate) fn new(table: AnyTable, capabilities: VistaCapabilities) -> Self {
        Self {
            table,
            capabilities,
        }
    }
}

#[async_trait]
impl TableShell for GraphqlApiTableShell {
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
        self.table.get_value(id).await
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        self.table.get_some_value().await
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        self.table.get_count().await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        // Vista hands us a CBOR scalar; coerce to its natural string form
        // and push through `AnyTable::add_condition_eq`, which routes back
        // to `GraphqlApi::eq_condition` and lands as a typed `_eq` filter.
        let s = match value {
            CborValue::Text(s) => s.clone(),
            CborValue::Integer(i) => i128::from(*i).to_string(),
            CborValue::Float(f) => f.to_string(),
            CborValue::Bool(b) => b.to_string(),
            CborValue::Null => String::new(),
            other => {
                return Err(error!(
                    "GraphqlApiTableShell: eq value must be scalar",
                    field = field,
                    value = format!("{:?}", other)
                ));
            }
        };
        self.table.add_condition_eq(field, &s)
    }

    fn get_ref(&self, _vista: &Vista, relation: &str, _row: &Record<CborValue>) -> Result<Vista> {
        // GraphQL still routes traversal through `AnyTable` because the
        // shell holds an `AnyTable` (the CBOR adapter is what bridges
        // `AnyGraphqlType` to `CborValue`). Cleaned up alongside the REST
        // refactor in Stage 9; for now the `row` parameter is ignored and
        // the legacy AnyTable-flavoured resolution runs.
        let any_table = self.table.get_ref(relation)?;
        AnyTableShell::into_vista(any_table)
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "graphql"
    }
}
