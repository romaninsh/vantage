//! Applying a Dio's residual to the rows its cache holds.
//!
//! The cache is filled from the master, which never applied the residual, so
//! it may hold rows outside the Dio. Every read path that serves rows from
//! the cache passes them through [`admitted_by_dio`], over rows hydrated
//! first when the residual names an augmented value.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::VistaQuery;

use super::DioInner;
use crate::lens::CacheTable;

/// Whether `record` is in `dio`: it satisfies the Dio's residual, which the
/// master never applied. A residual holds no native conditions (deriving a
/// Dio refuses them); should the evaluator fail anyway, the row is left out
/// and the failure logged.
pub(crate) fn admitted_by_dio(dio: &DioInner, record: &Record<CborValue>) -> bool {
    match dio.residual.matches(record, &dio.metadata) {
        Ok(admitted) => admitted,
        Err(e) => {
            tracing::warn!(
                target: "vantage_diorama::filter",
                table = %dio.master.name(),
                error = %e,
                "the Dio's residual can't be evaluated; the row is left out",
            );
            false
        }
    }
}

impl DioInner {
    /// The row `id` read through `reader`, or `None` when it is absent or
    /// outside this Dio's residual. A residual naming an augmented value
    /// judges the row hydrated (see [`DioInner::read_hydrated`]).
    pub(crate) async fn read_admitted(
        self: &Arc<Self>,
        reader: &dyn CacheTable,
        id: &str,
    ) -> Result<Option<Record<CborValue>>> {
        Ok(self
            .read_hydrated(reader, id, &VistaQuery::new())
            .await?
            .filter(|record| admitted_by_dio(self, record)))
    }
}
