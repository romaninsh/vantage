//! The registry of live [`SharedRows`] and the list of live table views.
//!
//! Both hold `Weak`s. A view holds its `SharedRows` strongly and the registry
//! doesn't, so a set of rows lives as long as some view points at it. When
//! the last view detaches, the rows' work is aborted at once (see
//! `SharedRows::detach`); a task the abort hasn't reached yet may keep the
//! `Arc` alive a moment longer, but such rows count as gone: the registry
//! never hands them back, and `SharedRows::try_attach` refuses them, so a
//! view never joins rows whose work was aborted.

use std::sync::{Arc, Weak};

use vantage_vista::VistaQuery;

use super::DioInner;
use crate::scenery::table::{SharedRows, TableView};

/// The rows behind `weak`, if some view still reads them.
fn read(weak: &Weak<SharedRows>) -> Option<Arc<SharedRows>> {
    weak.upgrade()
        .filter(|rows| !rows.views.lock().unwrap().is_empty())
}

/// The registry key of the rows answering `query`: the Dio's cache identity,
/// the query's order-insensitive key, and whether the rows are a titles-only
/// projection. Page size and demand are view settings and stay out of it.
pub(crate) fn shared_key(dio: &DioInner, query: &VistaQuery, titles_only: bool) -> String {
    format!(
        "{}\u{1}{}\u{1}{}",
        dio.cache_table_name,
        query.key(),
        titles_only as u8
    )
}

impl DioInner {
    /// The rows registered under `key`, if some view still reads them.
    pub(crate) fn shared_rows(&self, key: &str) -> Option<Arc<SharedRows>> {
        self.shared_rows.lock().unwrap().get(key).and_then(read)
    }

    /// Register `rows` under `key` and hand back the rows to use. When a
    /// concurrent open registered rows under the same key first, and some
    /// view reads them, those win and the caller drops `rows`, which aborts
    /// its tasks. An entry whose rows no view reads is overwritten; entries
    /// whose rows have dropped are pruned.
    pub(crate) fn register_shared_rows(
        &self,
        key: String,
        rows: Arc<SharedRows>,
    ) -> Arc<SharedRows> {
        let mut registry = self.shared_rows.lock().unwrap();
        if let Some(live) = registry.get(&key).and_then(read) {
            return live;
        }
        registry.retain(|_, weak| weak.strong_count() > 0);
        registry.insert(key, Arc::downgrade(&rows));
        rows
    }

    /// Add `view` to the live-view list, pruning closed views.
    pub(crate) fn register_view(&self, view: &Arc<TableView>) {
        let mut views = self.table_views.lock().unwrap();
        views.retain(|weak| weak.strong_count() > 0);
        views.push(Arc::downgrade(view));
    }

    /// Every table view with a live handle.
    pub(crate) fn live_views(&self) -> Vec<Arc<TableView>> {
        self.table_views
            .lock()
            .unwrap()
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    }
}
