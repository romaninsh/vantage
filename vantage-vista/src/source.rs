use std::pin::Pin;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use futures_core::Stream;
use indexmap::IndexMap;
use vantage_core::Result;
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

    async fn insert_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>>;

    async fn replace_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>>;

    async fn patch_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>>;

    async fn delete_vista_value(&self, vista: &Vista, id: &String) -> Result<()>;

    async fn delete_vista_all_values(&self, vista: &Vista) -> Result<()>;

    // ---- InsertableValueSet delegate ---------------------------------------

    async fn insert_vista_return_id_value(
        &self,
        vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String>;

    // ---- Aggregates --------------------------------------------------------

    async fn get_vista_count(&self, vista: &Vista) -> Result<i64>;

    // ---- Capability advertisement -----------------------------------------

    fn capabilities(&self) -> &VistaCapabilities;
}
