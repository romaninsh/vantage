//! Small fixed masters: the bucket (an object list augmented with per-object
//! detail, the canonical "cmd bucket of JSON" shape) and the teams list.

use std::sync::Arc;

use vantage_diorama::{Augmentation, Cache, Detail, Dio, Fetch, MergeRule, Source};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Catalog, Vista, VistaResolver};

use super::{augmented_dio, meta, quick_lens, record};

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
    Augmentation::new(
        Detail::Resolved {
            resolver: bucket_catalog(),
            name: "names".into(),
        },
        Source::Id,
        Fetch::PerRow,
        MergeRule::lift(["name"]),
    )
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
