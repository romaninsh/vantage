//! A paged search over a master that can search and window must return only
//! the matching rows.
//!
//! The loader fetches through `Dio::fetch_window_ordered_counted`, which
//! narrows a per-call clone of the master (`with_query`). In 1.x a master
//! that couldn't be cloned fell back to an un-narrowed window, dropping the
//! search without a word. In 2.0 every shell is cloneable, so the master
//! here is a plain `#[derive(Clone)]` wrapper and the search reaches the
//! source.

use std::sync::Arc;
use std::time::Duration;

use vantage_core::Result;
use vantage_diorama::{Lens, TableScenery};
use vantage_vista::{
    Capability, Page, TableShell, Vista, VistaCapabilities, forward_table_shell, mocks::MockShell,
};

use super::fixtures::eventually;
use crate::support::chunk::order;
use crate::support::{meta, record};

/// Bounds each wait on the view; not a speed assertion.
const WAIT: Duration = Duration::from_secs(10);

/// A searchable, windowable master that reports the total of its current
/// narrowing with every window, like a paged API's response envelope.
#[derive(Clone)]
struct PagedSearchShell {
    inner: Box<dyn TableShell>,
    capabilities: VistaCapabilities,
}

forward_table_shell!(PagedSearchShell, inner, {
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let page = self.inner.fetch_window(offset, limit).await?;
        let total = self.inner.count().await?;
        Ok(page.with_total(total))
    }
});

/// Five rows; only `axe` and `box` contain an "x".
fn paged_search_master() -> Vista {
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
    let capabilities = [Capability::Search, Capability::Window]
        .into_iter()
        .collect();
    Vista::new("items", inner).map_shell(|inner| PagedSearchShell {
        inner,
        capabilities,
    })
}

/// The names a scenery shows (sorted) with its total.
fn shown(scenery: &Arc<dyn TableScenery>) -> (Vec<String>, Option<usize>) {
    let mut names = order(scenery, "name");
    names.sort();
    (names, scenery.estimated_total())
}

#[tokio::test]
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
    let dio = lens.make_dio(paged_search_master()).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    assert!(scenery.search_supported(), "the master advertises Search");

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
