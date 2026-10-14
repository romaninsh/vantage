//! The rows an aggregation reduces: the source Dio's, as its readers see them.

use vantage_core::Result;
use vantage_vista::VistaQuery;

use crate::Dio;
use crate::aggregate::aggregation::{Reads, Rows};
use crate::dio::{DioInner, admitted_by_dio};

/// The Dio's row count straight from its cache, when `reads` is
/// [`Reads::Count`] and the residual doesn't narrow: every cached row is
/// then in the Dio, so no row is read or hydrated. `None` otherwise.
pub(crate) async fn cached_count(dio: &Dio, reads: &Reads) -> Result<Option<u64>> {
    if *reads != Reads::Count || dio.inner.residual_narrows() {
        return Ok(None);
    }
    let count = dio.inner.cache.count().await?;
    Ok(Some(count.max(0) as u64))
}

/// Every cached row the Dio's residual admits, in the Dio's order when it
/// has one. The rows are hydrated first when the residual or `reads` names
/// an augmented value, so neither judges, orders nor reduces a gap.
pub(crate) async fn admitted_rows(dio: &Dio, reads: &Reads) -> Result<Rows> {
    let inner = &dio.inner;
    let mut rows = inner.cache.list_values().await?;
    if needs_hydration(inner, reads) {
        inner.hydrate(&mut rows).await?;
    }
    rows.retain(|_, record| admitted_by_dio(inner, record));
    if inner.query.order.is_none() {
        return Ok(rows);
    }
    let mut ordered: Vec<_> = rows.into_iter().collect();
    inner.query.sort_rows(&mut ordered, &inner.metadata);
    Ok(ordered.into_iter().collect())
}

fn needs_hydration(inner: &DioInner, reads: &Reads) -> bool {
    if inner.names_augmented(&VistaQuery::new()) {
        return true;
    }
    match reads {
        Reads::Columns(columns) => columns.iter().any(|c| inner.may_be_augmented(c)),
        Reads::Rows => inner.has_dio_augment(),
        Reads::Count => false,
    }
}
