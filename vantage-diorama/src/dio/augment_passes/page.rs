//! The list pass's page read: from the cache's whole copy, or from the
//! master.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::Capability;

use crate::dio::Dio;
use crate::lens::CacheStatus;

/// One list page: its rows, and the set's total when it is known.
pub(crate) struct ListedPage {
    pub(crate) rows: Vec<(String, Record<CborValue>)>,
    /// Each row's cache status, in row order, for a page read from the
    /// cache; `None` for one from the master.
    pub(crate) statuses: Option<Vec<CacheStatus>>,
    pub(crate) total: Option<usize>,
}

/// Capability-aware **list pass**. From the cache (`from_cache`, its whole
/// copy), every row from `offset` in the cache's id order, in one read, so
/// the listing completes on the total. From the master, the
/// `[offset, offset+limit)` window pushed down when it advertises `Window`,
/// or the whole set listed and windowed locally so sequential paging still
/// terminates.
///
/// The total is the one the master states, or the whole list's length.
pub(crate) async fn list_page(
    dio: &Dio,
    offset: usize,
    limit: usize,
    from_cache: bool,
) -> Result<ListedPage> {
    if from_cache {
        let held = dio.inner.cache.list_values_with_status().await?;
        let total = held.len();
        let (rows, statuses) = held
            .into_iter()
            .skip(offset)
            .map(|(id, (record, status))| ((id, record), status))
            .unzip();
        return Ok(ListedPage {
            rows,
            statuses: Some(statuses),
            total: Some(total),
        });
    }
    let master = dio.master();
    if master.capabilities().has(Capability::Window) {
        let page = master.fetch_window(offset, limit).await?;
        Ok(ListedPage {
            total: page.total.and_then(|t| usize::try_from(t).ok()),
            rows: page.rows,
            statuses: None,
        })
    } else {
        let rows = master.list_values().await?;
        let total = rows.len();
        Ok(ListedPage {
            rows: rows.into_iter().skip(offset).take(limit).collect(),
            statuses: None,
            total: Some(total),
        })
    }
}
