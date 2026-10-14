//! Every setting `Lens::builder()` refuses, named by the error's `setting`
//! context, and a Dio build refused for a vista without an id column.

use std::time::Duration;

use vantage_core::VantageError;
use vantage_diorama::{Cache, Lens, LensBuilder, Loader, Refresher, Tuning};
use vantage_vista::Vista;
use vantage_vista::mocks::MockShell;

use crate::TTL;
use crate::support::{copy_master, memory_lens};

/// The setting a build error names, as the error's `setting` context.
fn setting(err: &VantageError) -> Option<&str> {
    err.context.get("setting").map(|s| s.trim_matches('"'))
}

fn refused(builder: LensBuilder) -> VantageError {
    match builder.build() {
        Ok(_) => panic!("the build was accepted"),
        Err(err) => err,
    }
}

fn memory() -> LensBuilder {
    Lens::builder().cache(Cache::memory())
}

fn noop_loader() -> Loader {
    Loader::paged(|_cx, _range, _query, _sink| async { Ok(()) })
}

#[tokio::test]
async fn a_lens_without_a_cache_is_refused() {
    assert_eq!(setting(&refused(Lens::builder())), Some("cache"));
}

#[test]
fn a_lens_outside_a_runtime_is_refused_not_panicked() {
    assert_eq!(setting(&refused(memory())), Some("runtime"));
}

#[test]
fn an_explicit_runtime_builds_outside_one() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert!(memory().runtime(runtime.handle().clone()).build().is_ok());
}

#[tokio::test]
async fn blocking_on_a_paged_loader_without_an_eager_start_is_refused() {
    let err = refused(memory().loader(noop_loader().blocking(true)));
    assert_eq!(setting(&err), Some("loader.blocking"));

    // With an eager start for the masters that can't page, it applies.
    let hybrid = noop_loader()
        .or_eager(|_cx| async { Ok(()) })
        .blocking(false);
    assert!(memory().loader(hybrid).build().is_ok());
}

#[tokio::test]
async fn or_eager_on_an_eager_loader_is_refused() {
    let loader = Loader::eager(|_cx| async { Ok(()) }).or_eager(|_cx| async { Ok(()) });
    assert_eq!(
        setting(&refused(memory().loader(loader))),
        Some("loader.or_eager")
    );
}

#[tokio::test]
async fn refresher_settings_are_validated() {
    let refresh = || Refresher::new(|_cx| async { Ok(()) });
    memory()
        .refresher(refresh().max_age(TTL))
        .build()
        .expect("max_age alone is accepted");
    let err = refused(memory().refresher(refresh().every(Duration::ZERO)));
    assert_eq!(setting(&err), Some("refresher.every"));
}

#[tokio::test]
async fn zero_sized_tuning_is_refused() {
    let err = refused(memory().tuning(Tuning::default().with_write_queue(0)));
    assert_eq!(setting(&err), Some("tuning.write_queue"));
    let err = refused(memory().tuning(Tuning::default().with_augment_workers(0)));
    assert_eq!(setting(&err), Some("tuning.augment_workers"));
}

#[tokio::test]
async fn a_redb_file_that_fails_to_open_fails_the_build() {
    // A directory can't be opened as a database file.
    let dir = tempfile::TempDir::new().unwrap();
    let err = refused(Lens::builder().cache(Cache::redb(dir.path())));
    assert_eq!(setting(&err), Some("cache"));
    assert!(
        std::error::Error::source(&err).is_some(),
        "redb's error is kept as the cause: {err}"
    );
}

/// Every cached row is keyed by the id column, so a vista without one is
/// refused when the Dio is built, naming the table.
#[tokio::test]
async fn a_vista_without_an_id_column_fails_the_build() {
    let master = Vista::new("keyless", MockShell::new());
    let err = memory_lens(copy_master())
        .dio(master)
        .build()
        .await
        .err()
        .expect("the build was accepted");
    assert!(err.to_string().contains("id column"), "{err}");
    assert_eq!(
        err.context.get("table").map(|s| s.trim_matches('"')),
        Some("keyless")
    );
}
