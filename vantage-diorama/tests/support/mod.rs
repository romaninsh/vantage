//! Shared test harness for the Dio-query-semantics work.
//!
//! - [`MockView`] — a consumer that talks ONLY over a `TableScenery`, the way a
//!   real grid/dropdown does: drive the viewport, read `gray_rows`/`loaded_rows`/
//!   `is_loading`/`total`, and settle on generation bumps. No gpui, no UI types.
//! - bucket fixtures — an object-list master augmented with per-object detail,
//!   the canonical "cmd bucket of JSON" shape.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Augmentation, Dio, Fetch, Lens, MergeRule, Source, TableScenery};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};
use vantage_vista_factory::VistaCatalog;

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

// ---- bucket fixture ------------------------------------------------------

/// Master: a bucket object list — ids only, no `name` (that's augmented).
pub fn bucket_master() -> Vista {
    let shell = MockShell::new()
        .with_record("o1", record(&[("id", "o1")]))
        .with_record("o2", record(&[("id", "o2")]))
        .with_record("o3", record(&[("id", "o3")]))
        .with_metadata(meta(&["id"]));
    Vista::new("bucket", Box::new(shell))
}

/// Detail source "names": object id -> { name }. 1st and 3rd are John.
pub fn names_detail() -> Vista {
    let shell = MockShell::new()
        .with_record("o1", record(&[("id", "o1"), ("name", "John")]))
        .with_record("o2", record(&[("id", "o2"), ("name", "Jane")]))
        .with_record("o3", record(&[("id", "o3"), ("name", "John")]))
        .with_metadata(meta(&["id", "name"]));
    Vista::new("names", Box::new(shell))
}

pub fn bucket_catalog() -> Arc<VistaCatalog> {
    let mut c = VistaCatalog::new();
    c.register("names", Arc::new(|| Ok(names_detail())));
    Arc::new(c)
}

pub fn augment_name() -> Augmentation {
    Augmentation {
        table: "names".into(),
        source: Source::Id,
        fetch: Fetch::PerRow,
        merge: MergeRule {
            columns: vec!["name".into()],
        },
    }
}

/// A Dio over the bucket master, augmenting each row's `name` from the `names`
/// detail source, backed by an in-memory cache (no TempDir). Augmentation is
/// configured on the **Dio** (`dio.augment`), not the Lens.
pub async fn bucket_dio() -> Dio {
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .viewport_debounce(Duration::from_millis(1))
            .build()
            .expect("lens builds"),
    );
    let dio = lens.make_dio(bucket_master()).await.expect("make_dio");
    dio.augment(bucket_catalog(), vec![augment_name()])
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
    Vista::new("members", Box::new(shell))
}

/// A single-pass Dio whose `on_start` eagerly copies the master into an
/// in-memory cache. No augmentation — the scenery filters/sorts the cache
/// locally (the v1-compat path).
pub async fn eager_dio(master: Vista) -> Dio {
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .build()
            .expect("lens builds"),
    );
    lens.make_dio(master).await.expect("make_dio")
}

// ---- MockView ------------------------------------------------------------

/// A scenery consumer for tests. Wraps a `TableScenery` and exposes the
/// observable state a grid/dropdown reacts to.
pub struct MockView {
    scenery: Arc<dyn TableScenery>,
}

impl MockView {
    /// Open a grid-style view over a Dio with the given page size.
    pub async fn open(dio: &Dio, page_size: usize) -> Self {
        let scenery = dio
            .table_scenery()
            .page_size(page_size)
            .open()
            .await
            .expect("scenery opens");
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
        self.scenery.set_viewport(range);
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
