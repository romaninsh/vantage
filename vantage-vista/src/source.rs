use std::pin::Pin;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use futures_core::Stream;
use indexmap::IndexMap;
use vantage_core::{Result, VantageError, error};
use vantage_types::Record;

use crate::{capabilities::VistaCapabilities, vista::Vista};

/// Per-driver executor for a `Vista`.
///
/// Implementations live in driver crates (vantage-sqlite, vantage-mongodb,
/// vantage-aws, etc.). Each method receives `&Vista` so the driver can read
/// the current condition state, columns, and other metadata.
///
/// `Id = String` and `Value = ciborium::Value` at this boundary, mirroring
/// `AnyTable`. CBOR-typed driver ids (Mongo `ObjectId`, Surreal `Thing`)
/// stringify here. Methods are named with the `_vista_` infix to mirror
/// `TableSource`'s `_table_` convention; `Vista`'s `ValueSet` impls
/// delegate by stripping the infix.
///
/// `id: &String` (rather than `&str`) is intentional: the upstream
/// `vantage_dataset::ValueSet` trait family fixes `Id = String` and uses
/// `&Self::Id` in its signatures, so impls receive `&String` and forward
/// it through unchanged.
#[async_trait]
#[allow(clippy::ptr_arg)]
pub trait VistaSource: Send + Sync + 'static {
    // ---- ReadableValueSet delegates ----------------------------------------

    async fn list_vista_values(&self, vista: &Vista)
    -> Result<IndexMap<String, Record<CborValue>>>;

    async fn get_vista_value(
        &self,
        vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>>;

    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>>;

    /// Default implementation wraps `list_vista_values`. Drivers with native
    /// streaming (cursor-based queries, paginated REST APIs) override.
    #[allow(clippy::type_complexity)]
    fn stream_vista_values<'a>(
        &'a self,
        vista: &'a Vista,
    ) -> Pin<Box<dyn Stream<Item = Result<(String, Record<CborValue>)>> + Send + 'a>>
    where
        Self: Sync,
    {
        Box::pin(async_stream::stream! {
            match self.list_vista_values(vista).await {
                Ok(map) => {
                    for item in map {
                        yield Ok(item);
                    }
                }
                Err(e) => yield Err(e),
            }
        })
    }

    // ---- WritableValueSet delegates ----------------------------------------
    //
    // Default impls return a typed VantageError via `default_error` — drivers
    // override only what they actually support. The matching `VistaCapabilities`
    // flag must be set to `true` for any method the driver implements; if the
    // flag is `true` but the trait method falls through to the default,
    // `default_error` produces an `Unimplemented`-kind error (placeholder
    // detected). If the flag is `false`, it produces `Unsupported`. Both are
    // emitted as tracing events at construction.

    async fn insert_vista_value(
        &self,
        vista: &Vista,
        _id: &String,
        _record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        Err(self.default_error("insert_vista_value", "can_insert", vista))
    }

    async fn replace_vista_value(
        &self,
        vista: &Vista,
        _id: &String,
        _record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        Err(self.default_error("replace_vista_value", "can_update", vista))
    }

    async fn patch_vista_value(
        &self,
        vista: &Vista,
        _id: &String,
        _partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        Err(self.default_error("patch_vista_value", "can_update", vista))
    }

    async fn delete_vista_value(&self, vista: &Vista, _id: &String) -> Result<()> {
        Err(self.default_error("delete_vista_value", "can_delete", vista))
    }

    async fn delete_vista_all_values(&self, vista: &Vista) -> Result<()> {
        Err(self.default_error("delete_vista_all_values", "can_delete", vista))
    }

    // ---- InsertableValueSet delegate ---------------------------------------

    async fn insert_vista_return_id_value(
        &self,
        vista: &Vista,
        _record: &Record<CborValue>,
    ) -> Result<String> {
        Err(self.default_error("insert_vista_return_id_value", "can_insert", vista))
    }

    // ---- Aggregates --------------------------------------------------------

    /// Default impl falls back to `list_vista_values` — drivers with native
    /// count (`SELECT COUNT(*)`, etc.) override.
    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        Ok(self.list_vista_values(vista).await?.len() as i64)
    }

    // ---- Capability advertisement -----------------------------------------

    fn capabilities(&self) -> &VistaCapabilities;

    /// Look up a capability flag by name. Used by `default_error` to decide
    /// between `Unsupported` and `Unimplemented`. Drivers don't normally
    /// need to override this.
    fn capability_flag(&self, name: &str) -> bool {
        let caps = self.capabilities();
        match name {
            "can_count" => caps.can_count,
            "can_insert" => caps.can_insert,
            "can_update" => caps.can_update,
            "can_delete" => caps.can_delete,
            "can_subscribe" => caps.can_subscribe,
            "can_invalidate" => caps.can_invalidate,
            _ => false,
        }
    }

    /// Build the standard error returned by default trait method impls.
    ///
    /// Picks the kind based on the capability flag: a `true` flag means the
    /// driver advertised support but didn't override the method (placeholder
    /// → `Unimplemented`); a `false` flag means the driver honestly doesn't
    /// claim the op (caller should have checked → `Unsupported`).
    ///
    /// Both kinds emit a `tracing::error!` at construction with `method`,
    /// `capability`, `source_type`, and `vista_name` as structured fields.
    fn default_error(&self, method: &str, capability: &str, vista: &Vista) -> VantageError {
        let source_type = std::any::type_name::<Self>();
        let vista_name = vista.name().to_string();
        if self.capability_flag(capability) {
            error!(
                format!(
                    "'{}' is advertised as VistaCapability for '{}' but implementation for '{}' is missing",
                    capability, source_type, method
                ),
                method = method,
                capability = capability,
                source_type = source_type,
                vista_name = vista_name
            )
            .is_unimplemented()
        } else {
            error!(
                format!(
                    "'{}' is not supported by '{}'; '{}' refused",
                    capability, source_type, method
                ),
                method = method,
                capability = capability,
                source_type = source_type,
                vista_name = vista_name
            )
            .is_unsupported()
        }
    }
}
