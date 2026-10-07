//! Shaped writes: forwarded when advertised, no toll — the scenarios stress
//! the read path; write latency is a personality nobody asked for yet.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::Vista;
use vantage_vista::source::TableShell;

use super::super::ShapedShell;

impl ShapedShell {
    pub(super) async fn shaped_insert(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.gate(
            self.shape.capabilities.can_insert,
            "insert_vista_value",
            "can_insert",
        )?;
        self.inner.insert_vista_value(vista, id, record).await
    }

    pub(super) async fn shaped_replace(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.gate(
            self.shape.capabilities.can_update,
            "replace_vista_value",
            "can_update",
        )?;
        self.inner.replace_vista_value(vista, id, record).await
    }

    /// A replace, then an insert when the row is missing, each gated.
    pub(super) async fn shaped_upsert(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        match TableShell::replace_vista_value(self, vista, id, record).await {
            Err(e) if e.is_not_found() => {
                TableShell::insert_vista_value(self, vista, id, record).await
            }
            other => other,
        }
    }

    pub(super) async fn shaped_patch(
        &self,
        vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.gate(
            self.shape.capabilities.can_update,
            "patch_vista_value",
            "can_update",
        )?;
        self.inner.patch_vista_value(vista, id, partial).await
    }

    pub(super) async fn shaped_delete(&self, vista: &Vista, id: &String) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_delete,
            "delete_vista_value",
            "can_delete",
        )?;
        self.inner.delete_vista_value(vista, id).await
    }

    pub(super) async fn shaped_delete_all(&self, vista: &Vista) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_delete,
            "delete_vista_all_values",
            "can_delete",
        )?;
        self.inner.delete_vista_all_values(vista).await
    }

    pub(super) async fn shaped_insert_return_id(
        &self,
        vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        self.gate(
            self.shape.capabilities.can_insert,
            "insert_vista_return_id_value",
            "can_insert",
        )?;
        self.inner.insert_vista_return_id_value(vista, record).await
    }
}
