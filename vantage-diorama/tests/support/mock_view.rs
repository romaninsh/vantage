//! [`MockView`]: a consumer that talks only over a `TableScenery`, the way a
//! real grid or dropdown does.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_diorama::{Dio, TableScenery};

use super::wait_listed;

/// A scenery consumer for tests. Wraps a `TableScenery` and exposes the
/// observable state a grid/dropdown reacts to.
pub struct MockView {
    scenery: Arc<dyn TableScenery>,
}

impl MockView {
    /// Open a grid-style view over a Dio with the given page size, and wait for
    /// its first rows — a real grid renders a loading state until then, so a
    /// test that asserts straight after `open()` is asserting about a frame the
    /// user never sees.
    pub async fn open(dio: &Dio, page_size: usize) -> Self {
        let scenery = dio
            .table_scenery()
            .page_size(page_size)
            .open()
            .await
            .expect("scenery opens");
        wait_listed(&scenery).await;
        Self { scenery }
    }

    /// Open a view, customizing the scenery builder first — e.g. to add a
    /// `filter` or an `order`.
    pub async fn open_with(
        dio: &Dio,
        page_size: usize,
        customize: impl FnOnce(
            vantage_diorama::TableSceneryBuilder,
        ) -> vantage_diorama::TableSceneryBuilder,
    ) -> Self {
        let builder = dio.table_scenery().page_size(page_size);
        let scenery = customize(builder).open().await.expect("scenery opens");
        wait_listed(&scenery).await;
        Self { scenery }
    }

    pub fn scenery(&self) -> &Arc<dyn TableScenery> {
        &self.scenery
    }

    /// Rows known to exist but not yet hydrated (list-pass `Incomplete`).
    pub fn gray_rows(&self) -> usize {
        self.scenery.status_summary().incomplete
    }

    /// Fully hydrated rows (`Fresh`).
    pub fn loaded_rows(&self) -> usize {
        self.scenery.status_summary().fresh
    }

    pub fn row_count(&self) -> usize {
        self.scenery.row_count()
    }

    /// Read a text column at a row index (None if absent / not a string).
    pub fn col_at(&self, idx: usize, col: &str) -> Option<String> {
        self.scenery.row(idx).and_then(|r| match r.record.get(col) {
            Some(CborValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
    }

    pub fn total(&self) -> Option<usize> {
        self.scenery.estimated_total()
    }

    /// Still settling: any gray rows, or fewer materialized rows than the count.
    pub fn is_loading(&self) -> bool {
        let s = self.scenery.status_summary();
        s.incomplete > 0 || s.loaded < self.scenery.row_count()
    }

    pub fn viewport(&self, range: std::ops::Range<usize>) {
        self.scenery.set_viewport(range, 0..0);
    }

    /// Pump until `pred` holds or a bounded budget elapses. Wall-clock based
    /// (not paused-time) — the BDD harness migration adds the paused variant.
    pub async fn settle_until(&self, label: &str, pred: impl Fn(&Self) -> bool) {
        for _ in 0..400 {
            if pred(self) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("MockView condition '{label}' not met within budget");
    }
}
