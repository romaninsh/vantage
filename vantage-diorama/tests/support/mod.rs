//! Shared test harness for the Dio-query-semantics work.
//!
//! - [`MockView`] — a consumer that talks ONLY over a `TableScenery`, the way a
//!   real grid/dropdown does: drive the viewport, read `gray_rows`/`loaded_rows`/
//!   `is_loading`/`total`, and settle on generation bumps. No gpui, no UI types.
//! - bucket fixtures — an object-list master augmented with per-object detail,
//!   the canonical "cmd bucket of JSON" shape.
//! - Lens builders — [`memory_lens`], [`eager_dio`], [`paged_dio`] — so a test
//!   states only the loader it cares about.

#![allow(dead_code)]

pub mod chunk;
pub mod flash;
pub mod spy_cache;
pub mod views;

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{
    Augmentation, Cache, Detail, Dio, Fetch, Lens, Loader, MergeRule, Source, TableScenery, Tuning,
};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Catalog, Column, Condition, FilterOp, SortDirection, Vista, VistaMetadata, VistaQuery,
    VistaResolver,
};

// ---- lens builders -------------------------------------------------------

/// A Lens over an in-memory cache with `loader`.
pub fn memory_lens(loader: Loader) -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::memory())
        .loader(loader)
        .build()
        .expect("lens builds")
}

/// An eager loader that copies the whole master into the cache.
pub fn copy_master() -> Loader {
    Loader::eager(copy_whole_master)
}

async fn copy_whole_master(cx: vantage_diorama::LoaderCx) -> vantage_core::Result<()> {
    let rows = cx.master().list_values().await?;
    cx.reconcile(rows).await
}

/// A paged loader that reads each window from the master, narrowed by the
/// view's query, and reports the master's total. A clause the master refuses
/// fails the load: this loader can't evaluate it over a window.
pub fn page_master() -> Loader {
    Loader::paged(|cx, range, query, sink| async move {
        let fetched = cx.fetch(range.clone(), &query).await?;
        if !fetched.refused.is_empty() {
            return Err(vantage_core::error!(
                "the master refused part of the view's query",
                refused = fetched.refused
            ));
        }
        let page = fetched.page;
        if let Some(total) = page.total {
            sink.set_total(total.max(0) as usize);
        }
        for (i, (id, record)) in page.rows.into_iter().enumerate() {
            sink.push(range.start + i, id, record).await?;
        }
        Ok(())
    })
}

/// A Dio over `master` whose cache is filled once by copying the master.
pub async fn eager_dio(master: Vista) -> Dio {
    memory_lens(copy_master())
        .dio(master)
        .build()
        .await
        .expect("dio builds")
}

/// A Dio over `master` whose table sceneries page windows from the master.
pub async fn paged_dio(master: Vista) -> Dio {
    memory_lens(page_master())
        .dio(master)
        .build()
        .await
        .expect("dio builds")
}

/// A Dio over `master` whose table sceneries page windows from the master,
/// and copy the whole master through an eager start when a view's query
/// leaves a clause the master can't apply. The master must advertise
/// `Window`, or every view is copied whole.
pub async fn paged_or_eager_dio(master: Vista) -> Dio {
    memory_lens(page_master().or_eager(copy_whole_master))
        .dio(master)
        .build()
        .await
        .expect("dio builds")
}

/// A Lens over `cache` with no loader and a 1ms viewport debounce: the shape
/// the augmentation tests drive, where the Dio's own list pass fills the
/// cache.
pub fn quick_lens(cache: Cache) -> Arc<Lens> {
    Lens::builder()
        .cache(cache)
        .tuning(Tuning::default().with_viewport_debounce(Duration::from_millis(1)))
        .build()
        .expect("lens builds")
}

/// A Dio over `master` on `lens`, augmented by `augmentations`.
pub async fn augmented_dio(
    lens: &Arc<Lens>,
    master: Vista,
    augmentations: Vec<Augmentation>,
) -> Dio {
    lens.dio(master)
        .augment(augmentations)
        .build()
        .await
        .expect("dio builds")
}

/// An augmentation reading `columns` from `detail`, a fixed Vista no
/// resolver knows, keyed by the master id.
pub fn fixed_augmentation(detail: Vista, columns: &[&str]) -> Augmentation {
    Augmentation {
        detail: Detail::Fixed(Arc::new(detail)),
        source: Source::Id,
        fetch: Fetch::PerRow,
        merge: MergeRule {
            columns: columns.iter().map(|c| c.to_string()).collect(),
        },
    }
}

/// A detail source keyed by id with columns `id` and `size`: `get` answers
/// from `sizes` (`(id, size)` pairs), while `list`, `count` and
/// `fetch_window` refuse. Its `get_log` records every lookup.
pub fn size_detail<'a>(sizes: impl IntoIterator<Item = (&'a str, &'a str)>) -> MockShell {
    sizes
        .into_iter()
        .fold(
            MockShell::new().with_metadata(meta(&["id", "size"])),
            |shell, (id, size)| shell.with_record(id, record(&[("id", id), ("size", size)])),
        )
        .get_only()
}

// ---- waiting -------------------------------------------------------------

/// Poll `holds` every 5ms until it is true or `within` elapses. Returns
/// whether it came true. Bounds a wait; it is not a speed assertion.
pub async fn eventually(within: Duration, holds: impl Fn() -> bool) -> bool {
    tokio::time::timeout(within, async {
        while !holds() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok()
}

// ---- record/metadata helpers ---------------------------------------------

pub fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

pub fn record(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), text(v)))
        .collect()
}

pub fn meta(columns: &[&str]) -> VistaMetadata {
    let mut m = VistaMetadata::new();
    for c in columns {
        let col = if *c == "id" {
            Column::new("id", "String").with_flag("id")
        } else {
            Column::new(*c, "String")
        };
        m = m.with_column(col);
    }
    m.with_id_column("id")
}

// ---- one-clause queries, for `Dio::with_query` ----------------------------

pub fn cond(column: &str, op: FilterOp, value: impl Into<CborValue>) -> VistaQuery {
    VistaQuery::new().with_condition(Condition::new(column, op, value))
}

pub fn search(text: &str) -> VistaQuery {
    VistaQuery::new().with_search(text)
}

pub fn order(column: &str, dir: SortDirection) -> VistaQuery {
    VistaQuery::new().with_order(column, dir)
}

// ---- bucket fixture ------------------------------------------------------

/// Master: a bucket object list. `kind` is a native (list-pass) column; `name`
/// is NOT present here — it's augmented from the `names` detail source.
pub fn bucket_master() -> Vista {
    let shell = MockShell::new()
        .with_record("o1", record(&[("id", "o1"), ("kind", "red")]))
        .with_record("o2", record(&[("id", "o2"), ("kind", "blue")]))
        .with_record("o3", record(&[("id", "o3"), ("kind", "red")]))
        .with_metadata(meta(&["id", "kind"]));
    Vista::new("bucket", shell)
}

/// Detail source "names": object id -> { name }. 1st and 3rd are John.
pub fn names_detail() -> Vista {
    let shell = MockShell::new()
        .with_record("o1", record(&[("id", "o1"), ("name", "John")]))
        .with_record("o2", record(&[("id", "o2"), ("name", "Jane")]))
        .with_record("o3", record(&[("id", "o3"), ("name", "John")]))
        .with_metadata(meta(&["id", "name"]));
    Vista::new("names", shell)
}

pub fn bucket_catalog() -> Arc<dyn VistaResolver> {
    Arc::new(Catalog::new().with("names", |_: &str| Ok(names_detail())))
}

pub fn augment_name() -> Augmentation {
    Augmentation {
        detail: Detail::Resolved {
            resolver: bucket_catalog(),
            name: "names".into(),
        },
        source: Source::Id,
        fetch: Fetch::PerRow,
        merge: MergeRule {
            columns: vec!["name".into()],
        },
    }
}

/// A Dio over the bucket master, augmenting each row's `name` from the `names`
/// detail source, backed by an in-memory cache (no TempDir). Augmentation is
/// configured on the **Dio** (`DioBuilder::augment`), not the Lens.
pub async fn bucket_dio() -> Dio {
    augmented_dio(
        &quick_lens(Cache::memory()),
        bucket_master(),
        vec![augment_name()],
    )
    .await
}

// ---- single-pass (eager) fixture -----------------------------------------

/// Master with a native `team` column — 2 red, 1 blue — for condition/order
/// tests that don't involve augmentation.
pub fn teams_master() -> Vista {
    let shell = MockShell::new()
        .with_record("a", record(&[("id", "a"), ("team", "red")]))
        .with_record("b", record(&[("id", "b"), ("team", "blue")]))
        .with_record("c", record(&[("id", "c"), ("team", "red")]))
        .with_metadata(meta(&["id", "team"]));
    Vista::new("members", shell)
}

/// Open a two-pass scenery and wait until its first rows have landed.
///
/// `open()` no longer implies data: a two-pass view runs its list page detached
/// so a slow source cannot freeze the caller (a UI thread, in the app). Waiting
/// on [`LoadState`] is the supported way to say "once it has something" —
/// a fixed sleep would be a speed assertion, and asserting straight after
/// `open()` is a race the test would sometimes win.
pub async fn open_listed(dio: &Dio, page_size: usize) -> Arc<dyn TableScenery> {
    let scenery = dio
        .table_scenery()
        .page_size(page_size)
        .open()
        .await
        .expect("scenery opens");
    wait_listed(&scenery).await;
    scenery
}

/// Wait for a scenery to leave [`LoadState::Loading`].
///
/// Driven by the generation watch rather than a poll: every path that settles a
/// scenery bumps the generation immediately afterwards, so the signal already
/// exists and sleeping between checks would only add latency to it. Subscribe
/// before the first check, or a load that finishes in between is a bump nobody
/// is listening for.
///
/// The timeout bounds a hang; it is not an assertion about speed.
pub async fn wait_listed(scenery: &Arc<dyn TableScenery>) {
    let mut generations = scenery.subscribe();
    let settled = async {
        while scenery.load_state() == vantage_diorama::LoadState::Loading {
            generations
                .changed()
                .await
                .expect("generation watch closed");
        }
    };
    tokio::time::timeout(Duration::from_secs(3), settled)
        .await
        .expect("scenery never left LoadState::Loading");
}

// ---- MockView ------------------------------------------------------------

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
