use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{Column, Reference, TableShell, Vista, VistaCapabilities};

use crate::dio::shell::DioShell;

#[async_trait]
impl TableShell for DioShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        self.dio.master.source.columns()
    }

    fn references(&self) -> &IndexMap<String, Reference> {
        self.dio.master.source.references()
    }

    fn id_column(&self) -> Option<&str> {
        self.dio.master.source.id_column()
    }

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        Err(self.default_error("list_vista_values", "can_count"))
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        _id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        Err(self.default_error("get_vista_value", "can_count"))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        Err(self.default_error("get_vista_some_value", "can_count"))
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "dio"
    }
}
