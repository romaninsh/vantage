//! [`EmptyShell`] and [`Vista::empty`]: a table with no columns, no rows
//! and no capabilities.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use crate::{Column, Reference, TableShell, Vista, VistaCapabilities};

type Rec = Record<CborValue>;

/// A [`TableShell`] with no schema, no id column, no rows and no
/// capabilities: every read answers empty, every write is refused as
/// unsupported. For a Vista that a caller needs but that stands for no
/// table, such as the table of a detached row.
#[derive(Default)]
pub struct EmptyShell {
    columns: IndexMap<String, Column>,
    references: IndexMap<String, Reference>,
    capabilities: VistaCapabilities,
}

impl Vista {
    /// A Vista named `name` over an [`EmptyShell`].
    pub fn empty(name: impl Into<String>) -> Vista {
        Vista::new(name, Box::new(EmptyShell::default()))
    }
}

#[async_trait]
impl TableShell for EmptyShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        &self.columns
    }
    fn references(&self) -> &IndexMap<String, Reference> {
        &self.references
    }
    fn id_column(&self) -> Option<&str> {
        None
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    fn driver_name(&self) -> &'static str {
        "empty"
    }
    fn preview_query(&self, _vista: &Vista) -> serde_json::Value {
        serde_json::json!({ "driver": "empty", "query": null, "note": "no table: nothing is queried" })
    }
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(EmptyShell::default()))
    }

    async fn list_vista_values(&self, _vista: &Vista) -> Result<IndexMap<String, Rec>> {
        Ok(IndexMap::new())
    }
    async fn get_vista_value(&self, _vista: &Vista, _id: &String) -> Result<Option<Rec>> {
        Ok(None)
    }
    async fn get_vista_some_value(&self, _vista: &Vista) -> Result<Option<(String, Rec)>> {
        Ok(None)
    }
    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};

    use super::*;

    #[tokio::test]
    async fn empty_vista_has_no_schema_rows_or_writes() {
        let vista = Vista::empty("row");
        assert_eq!(vista.name(), "row");
        assert!(vista.get_column_names().is_empty());
        assert_eq!(vista.get_id_column(), None);
        assert!(!vista.capabilities().can_insert);
        assert!(vista.list_values().await.unwrap().is_empty());
        assert_eq!(vista.get_value("x".to_string()).await.unwrap(), None);
        let err = vista
            .insert_value("x".to_string(), &Rec::new())
            .await
            .unwrap_err();
        assert!(err.is_unsupported(), "{err}");
    }
}
