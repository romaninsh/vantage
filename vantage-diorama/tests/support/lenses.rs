//! Lens and Dio builders, so a test states only the loader it cares about,
//! and the augmentations the two-pass tests wire in.

use std::sync::Arc;
use std::time::Duration;

use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{
    Augmentation, Cache, Detail, Dio, Fetch, Lens, Loader, MergeRule, Source, Tuning,
};
use vantage_vista::Vista;
use vantage_vista::mocks::MockShell;

use super::{meta, record};

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
    Augmentation::new(
        Detail::Fixed(Arc::new(detail)),
        Source::Id,
        Fetch::PerRow,
        MergeRule::lift(columns.iter().copied()),
    )
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
