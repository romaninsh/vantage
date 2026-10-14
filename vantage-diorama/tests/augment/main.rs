//! Generic augmentation, end-to-end: a master Vista listed cheaply, each row
//! enriched one-at-a-time from a *separate* detail Vista resolved through a
//! [`Catalog`]. Proves `DioBuilder::augment` drives the two-pass machinery
//! without any hand-written list/detail callbacks.
//!
//! Both vistas are in-memory `MockShell`s — independent handles resolved by
//! name, which is exactly the "wire two generic Vistas into one Dio" shape (the
//! same path serves a REST master + cmd detail, since the catalog is
//! persistence-agnostic).

mod demand;
mod refresh;
mod sources;
#[path = "../support/mod.rs"]
mod support;

use std::sync::Arc;

use ciborium::Value as CborValue;
use support::{
    augmented_dio, fixed_augmentation, meta, poll_until, quick_lens, record, size_detail, status_of,
};
use tempfile::TempDir;
use vantage_diorama::{
    Augmentation, Cache, Detail, Dio, Fetch, Lens, MergeRule, RowStatus, Source, TableScenery,
};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Catalog, Vista, VistaResolver};

/// Master: two rows with cheap columns only (id, branch).
fn master_vista() -> Vista {
    let shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("branch", "main")]))
        .with_record("r1", record(&[("id", "r1"), ("branch", "dev")]))
        .with_metadata(meta(&["id", "branch"]));
    Vista::new("runs", shell)
}

/// Detail source #1: keyed by id, carries the expensive `detail` column.
fn detail_vista() -> Vista {
    let shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("detail", "full-r0")]))
        .with_record("r1", record(&[("id", "r1"), ("detail", "full-r1")]))
        .with_metadata(meta(&["id", "detail"]));
    Vista::new("runs-detail", shell)
}

/// Detail source #2: a *second* independent vista, carries `extra`.
fn extra_vista() -> Vista {
    let shell = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("extra", "x0")]))
        .with_record("r1", record(&[("id", "r1"), ("extra", "x1")]))
        .with_metadata(meta(&["id", "extra"]));
    Vista::new("runs-extra", shell)
}

fn catalog() -> Arc<dyn VistaResolver> {
    Arc::new(
        Catalog::new()
            .with("runs-detail", |_: &str| Ok(detail_vista()))
            .with("runs-extra", |_: &str| Ok(extra_vista())),
    )
}

fn resolved(table: &str) -> Detail {
    Detail::Resolved {
        resolver: catalog(),
        name: table.into(),
    }
}

fn aug(table: &str, source: Source, merge: &[&str]) -> Augmentation {
    Augmentation::new(
        resolved(table),
        source,
        Fetch::PerRow,
        MergeRule::lift(merge.iter().copied()),
    )
}

/// A no-loader Lens over a redb file in `tmp`.
fn redb_lens(tmp: &TempDir) -> Arc<Lens> {
    quick_lens(Cache::redb(tmp.path().join("cache.redb")))
}

async fn open(tmp: &TempDir, augmentations: Vec<Augmentation>) -> Dio {
    augmented_dio(&redb_lens(tmp), master_vista(), augmentations).await
}

fn col_of(s: &Arc<dyn TableScenery>, i: usize, c: &str) -> Option<String> {
    s.row(i).and_then(|r| match r.record.get(c) {
        Some(CborValue::Text(t)) => Some(t.clone()),
        _ => None,
    })
}

/// Open a two-row listing over `dio`, ask for both rows, and wait until row
/// `row` is `Fresh`.
async fn hydrated(dio: &Dio, row: usize) -> Arc<dyn TableScenery> {
    let scenery = support::open_listed(dio, 2).await;
    scenery.set_viewport(0..2, 0..0);
    poll_until("hydrated", || {
        matches!(status_of(&scenery, row), Some(RowStatus::Fresh))
    })
    .await;
    scenery
}

/// A Dio over `master_shell` whose `size` comes from a fixed get-only detail
/// holding the `logs` and `tmp` folder sizes. The detail shell comes back too,
/// so a test can count its gets.
async fn folder_size_dio(tmp: &TempDir, master_shell: MockShell) -> (Dio, MockShell) {
    let detail = size_detail([("logs", "4096"), ("tmp", "512")]);
    let dio = augmented_dio(
        &redb_lens(tmp),
        Vista::new("listing", master_shell),
        vec![fixed_augmentation(
            Vista::new("sizes", detail.clone()),
            &["size"],
        )],
    )
    .await;
    (dio, detail)
}

/// A row's column by id, order-independent (reseeds re-list the cache).
fn col_by_id(s: &Arc<dyn TableScenery>, id: &str, c: &str) -> Option<String> {
    (0..s.row_count())
        .filter_map(|i| s.row(i))
        .find(|r| matches!(r.record.get("id"), Some(CborValue::Text(t)) if t == id))
        .and_then(|r| match r.record.get(c) {
            Some(CborValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
}
