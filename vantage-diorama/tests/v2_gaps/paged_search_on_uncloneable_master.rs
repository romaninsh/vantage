//! A paged search over a master that can search and window, but cannot
//! clone its shell, must still return only the matching rows.
//!
//! The loader fetches through `Dio::fetch_window_ordered_counted`, which
//! narrows a per-call clone of the master. With no clone it falls back to an
//! un-narrowed window, so the search is dropped without a word. In 2.0 a
//! Vista derives narrowed copies by value (`with_query_partial`), so a clone
//! is always available and the search reaches the source.
//!
//! A shell that can't be cloned can't exist in 2.0. Phase 1 turns
//! `UncloneableShell` into a `#[derive(Clone)]` wrapper; the assertion
//! stays the same.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_diorama::{Lens, TableScenery};
use vantage_types::Record;
use vantage_vista::reference::Reference;
use vantage_vista::source::TableShell;
use vantage_vista::{Column, Vista, VistaCapabilities, mocks::MockShell};

use super::fixtures::eventually;
use crate::support::chunk::order;
use crate::support::{meta, record};

/// Bounds each wait on the view; not a speed assertion.
const WAIT: Duration = Duration::from_secs(10);

/// A searchable, windowable master that leaves `clone_shell` at its default
/// (`None`), the way several real drivers do today.
struct UncloneableShell {
    inner: MockShell,
    capabilities: VistaCapabilities,
}

#[async_trait]
#[allow(clippy::ptr_arg)]
impl TableShell for UncloneableShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        self.inner.columns()
    }
    fn references(&self) -> &IndexMap<String, Reference> {
        self.inner.references()
    }
    fn id_column(&self) -> Option<&str> {
        self.inner.id_column()
    }
    async fn list_vista_values(
        &self,
        vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.inner.list_vista_values(vista).await
    }
    async fn get_vista_value(
        &self,
        vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        self.inner.get_vista_value(vista, id).await
    }
    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        self.inner.get_vista_some_value(vista).await
    }
    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        self.inner.get_vista_count(vista).await
    }
    async fn fetch_window(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        self.inner.fetch_window(vista, offset, limit).await
    }
    /// Reports the total of the shell's current narrowing with every window,
    /// like a paged API's response envelope.
    async fn fetch_window_counted(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<i64>)> {
        let rows = self.inner.fetch_window(vista, offset, limit).await?;
        let total = self.inner.get_vista_count(vista).await?;
        Ok((rows, Some(total)))
    }
    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        self.inner.add_eq_condition(field, value)
    }
    fn add_search(&mut self, text: &str) -> Result<()> {
        self.inner.add_search(text)
    }
    fn clear_search(&mut self) -> Result<()> {
        self.inner.clear_search()
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    fn preview_query(&self, vista: &Vista) -> serde_json::Value {
        self.inner.preview_query(vista)
    }
}

/// Five rows; only `axe` and `box` contain an "x".
fn uncloneable_master() -> Vista {
    let mut inner = MockShell::new().with_metadata(meta(&["id", "name"]));
    for (id, name) in [
        ("r1", "apple"),
        ("r2", "box"),
        ("r3", "axe"),
        ("r4", "cat"),
        ("r5", "dog"),
    ] {
        inner = inner.with_record(id, record(&[("id", id), ("name", name)]));
    }
    let capabilities = VistaCapabilities {
        can_count: true,
        can_search: true,
        can_fetch_window: true,
        ..Default::default()
    };
    Vista::new(
        "items",
        Box::new(UncloneableShell {
            inner,
            capabilities,
        }),
    )
}

/// The names a scenery shows (sorted) with its total.
fn shown(scenery: &Arc<dyn TableScenery>) -> (Vec<String>, Option<usize>) {
    let mut names = order(scenery, "name");
    names.sort();
    (names, scenery.estimated_total())
}

#[tokio::test]
#[ignore = "v2 phase 1: shell not cloneable, so the paged loader drops the search"]
async fn paged_search_on_uncloneable_master() -> Result<()> {
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .viewport_debounce(Duration::from_millis(1))
            .on_load_chunk(|dio, range, query, sink| {
                let dio = dio.clone();
                async move {
                    let (rows, total) = dio
                        .fetch_window_ordered_counted(range.start, range.len(), query)
                        .await?;
                    if let Some(total) = total {
                        sink.set_total(total as usize);
                    }
                    for (offset, (id, row)) in rows.into_iter().enumerate() {
                        sink.push(range.start + offset, id, row).await?;
                    }
                    Ok(())
                }
            })
            .build()
            .expect("lens builds"),
    );
    let dio = lens.make_dio(uncloneable_master()).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    assert!(
        scenery.search_supported(),
        "the master advertises can_search"
    );

    scenery.set_viewport(0..10);
    assert!(
        eventually(WAIT, || scenery.row_count() == 5).await,
        "the unsearched window never loaded"
    );

    scenery.set_search(Some("x".into()));
    scenery.set_viewport(0..10);
    let expected = (vec!["axe".to_string(), "box".to_string()], Some(2));
    eventually(WAIT, || shown(&scenery) == expected).await;
    assert_eq!(
        shown(&scenery),
        expected,
        "search \"x\" must return only the matching rows and the matching total"
    );
    Ok(())
}
