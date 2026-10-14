//! Writers that misbehave or can't be judged: one that flashes its own Dio,
//! one that panics, and one over a natively narrowed master.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, ChangeFlash, Dio, FlashOutcome, Lens, LoaderCx};
use vantage_vista::{NativeCondition, TableShell, Vista, forward_table_shell};

use super::{items, name_of, rename, seeded};
use crate::support::record;

/// A writer that flashes the Dio it is writing for gets a plain error naming
/// the Dio instead of waiting on itself forever.
#[tokio::test]
async fn a_writer_flashing_its_own_dio_is_refused() -> Result<()> {
    let own: Arc<OnceLock<Dio>> = Arc::default();
    let inner_error: Arc<Mutex<Option<String>>> = Arc::default();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer({
            let (own, inner_error) = (own.clone(), inner_error.clone());
            move |_cx, _flash| {
                let (own, inner_error) = (own.clone(), inner_error.clone());
                async move {
                    let dio = own.get().expect("dio set").clone();
                    let audit = ChangeFlash::insert("audit", record(&[("id", "audit")]));
                    if let Err(e) = dio.flash(audit).await {
                        *inner_error.lock().unwrap() = Some(e.to_string());
                    }
                    Ok(FlashOutcome::applied())
                }
            }
        })
        .build()?;
    let dio = seeded(&lens, Vista::new("items", items())).await?;
    own.set(dio.clone()).ok();

    tokio::time::timeout(Duration::from_secs(5), dio.flash(rename("edited")))
        .await
        .expect("the write must not hang")?;
    let message = inner_error
        .lock()
        .unwrap()
        .clone()
        .expect("the inner flash failed");
    assert!(
        message.contains("cannot flash the Dio it is writing for"),
        "{message}"
    );
    assert!(message.contains("items"), "names the Dio: {message}");
    Ok(())
}

/// A writer that panics fails its flash, which rolls back; the worker lives
/// on and takes the next write.
#[tokio::test]
async fn a_panicking_writer_rolls_back_and_the_queue_survives() -> Result<()> {
    let panic_next = Arc::new(AtomicBool::new(true));
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer({
            let panic_next = panic_next.clone();
            move |_cx: LoaderCx, _flash| {
                let panic_now = panic_next.swap(false, Ordering::SeqCst);
                async move {
                    assert!(!panic_now, "the writer panics on purpose");
                    Ok(FlashOutcome::applied())
                }
            }
        })
        .build()?;
    let dio = seeded(&lens, Vista::new("items", items())).await?;

    let err = dio.flash(rename("lost")).await.unwrap_err();
    assert!(err.to_string().contains("panicked"), "{err}");
    let row = dio.cached().get_value("a").await?;
    assert_eq!(name_of(row).as_deref(), Some("one"), "rolled back");

    dio.flash(rename("kept")).await?;
    let row = dio.cached().get_value("a").await?;
    assert_eq!(
        name_of(row).as_deref(),
        Some("kept"),
        "the worker took the next write"
    );
    Ok(())
}

/// Takes any native condition.
#[derive(Clone)]
struct Native {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Native, inner, {
    fn add_native_condition(&mut self, _condition: NativeCondition) -> Result<()> {
        Ok(())
    }
});

/// Under a writer, which bypasses the master's own confinement, a native
/// condition can't be judged in memory, so every write is refused with an
/// error naming the clause and the Dio.
#[tokio::test]
async fn a_writer_over_a_native_narrowing_refuses_writes() -> Result<()> {
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer(|_cx, _flash| async { Ok(FlashOutcome::applied()) })
        .build()?;
    let master = Vista::new("items", items())
        .map_shell(|inner| Native { inner })
        .with_native_condition(NativeCondition::new((), "is_paying = true"))?;
    let dio = lens.dio(master).build().await?;

    let err = dio.flash(rename("edited")).await.unwrap_err();
    let message = err.to_string();
    assert!(message.contains("native condition"), "{message}");
    assert!(
        message.contains("is_paying = true"),
        "names the clause: {message}"
    );
    assert!(message.contains("items"), "names the Dio: {message}");
    Ok(())
}
