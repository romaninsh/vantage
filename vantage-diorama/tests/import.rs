//! `Dio::import_values` — the bulk path behind `tables.<t>.import(…)`.
//!
//! The capability split under test: `Import` advertises a
//! driver-native all-or-nothing batch; without it the Dio falls back to
//! per-record optimistic inserts, where partial progress is honest —
//! an import that stops at row 3 says so, names the row, and leaves
//! rows 1–2 legitimately stored.

use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, Dio, FlashOutcome, Lens};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell,
};

mod support;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn tag_record(code: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("id".to_string(), text(code));
    r.insert("status".to_string(), text("unregistered"));
    r
}

fn tag_records(codes: &[&str]) -> IndexMap<String, Record<CborValue>> {
    codes
        .iter()
        .map(|code| (code.to_string(), tag_record(code)))
        .collect()
}

fn tag_vista(shell: &MockShell) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("status", "String"))
        .with_id_column("id");
    Vista::new("tags", shell.clone().with_metadata(metadata))
}

async fn dio_over(shell: &MockShell) -> Result<Dio> {
    support::quick_lens(Cache::memory())
        .dio(tag_vista(shell))
        .build()
        .await
}

#[tokio::test]
async fn fallback_imports_every_record_and_reports_progress() -> Result<()> {
    let shell = MockShell::new();
    let dio = dio_over(&shell).await?;

    let progress: Arc<Mutex<Vec<(usize, usize)>>> = Arc::default();
    let seen = progress.clone();
    let stored = dio
        .import_values(tag_records(&["t1", "t2", "t3"]), move |done, total| {
            seen.lock().unwrap().push((done, total));
            ControlFlow::Continue(())
        })
        .await?;

    assert_eq!(stored.inserted, 3);
    assert_eq!(stored.skipped, 0);
    assert!(!stored.cancelled);
    assert_eq!(
        *progress.lock().unwrap(),
        vec![(1, 3), (2, 3), (3, 3)],
        "one tick per completed row"
    );
    let upstream = dio.master().list_values().await?;
    assert_eq!(upstream.len(), 3, "every record reached the master");
    assert_eq!(
        upstream.get("t2").unwrap().get("status"),
        Some(&text("unregistered"))
    );
    assert!(
        dio.cached().get_value("t3").await?.is_some(),
        "records are visible through the cache"
    );
    Ok(())
}

/// An id the master already holds is skipped and NOT counted — the
/// driver's idempotent insert would otherwise report it as landed, and
/// a re-imported file would claim every row.
#[tokio::test]
async fn fallback_skips_existing_ids_and_counts_only_inserts() -> Result<()> {
    let mut existing = tag_record("t2");
    existing.insert("status".to_string(), text("registered"));
    let shell = MockShell::new().with_record("t2", existing);
    let dio = dio_over(&shell).await?;

    let progress: Arc<Mutex<Vec<(usize, usize)>>> = Arc::default();
    let seen = progress.clone();
    let inserted = dio
        .import_values(tag_records(&["t1", "t2", "t3"]), move |done, total| {
            seen.lock().unwrap().push((done, total));
            ControlFlow::Continue(())
        })
        .await?;

    assert_eq!(inserted.inserted, 2);
    assert_eq!(
        inserted.skipped, 1,
        "the caller is told what it already had, not left to subtract"
    );
    assert_eq!(inserted.processed(), 3);
    assert_eq!(
        *progress.lock().unwrap(),
        vec![(1, 3), (2, 3), (3, 3)],
        "progress covers the skipped row too"
    );
    assert_eq!(
        shell.get_record("t2").unwrap().get("status"),
        Some(&text("registered")),
        "the existing record is left as it was"
    );
    Ok(())
}

#[tokio::test]
async fn fallback_stops_at_first_failure_and_names_the_row() -> Result<()> {
    let shell = MockShell::new();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer(|_cx, flash| async move {
            if flash.id() == Some("t3") {
                return Err(vantage_core::error!("route rejected the flash"));
            }
            Ok(FlashOutcome::applied())
        })
        .build()
        .expect("build lens");
    let dio = lens.dio(tag_vista(&shell)).build().await?;

    let progress: Arc<Mutex<Vec<(usize, usize)>>> = Arc::default();
    let seen = progress.clone();
    let result = dio
        .import_values(
            tag_records(&["t1", "t2", "t3", "t4", "t5"]),
            move |done, total| {
                seen.lock().unwrap().push((done, total));
                ControlFlow::Continue(())
            },
        )
        .await;

    let err = result.expect_err("row 3 fails");
    let message = err.to_string();
    assert!(
        message.contains("row: 3, of: 5") && message.contains("\"t3\""),
        "the error names the failing row and id: {message}"
    );
    assert_eq!(
        *progress.lock().unwrap(),
        vec![(1, 5), (2, 5)],
        "progress reported exactly what landed before the stop"
    );
    assert!(
        dio.cached().get_value("t3").await?.is_none(),
        "the failed row's optimistic stage was rolled back"
    );
    assert!(
        dio.cached().get_value("t4").await?.is_none(),
        "nothing after the failure was attempted"
    );
    Ok(())
}

/// The progress callback is the stop button: an import is a long walk
/// over the network, and `Break` has to end it at the row it reaches —
/// what already landed stays landed, and the outcome admits it stopped.
#[tokio::test]
async fn breaking_from_progress_stops_the_walk() -> Result<()> {
    let shell = MockShell::new();
    let dio = dio_over(&shell).await?;

    let outcome = dio
        .import_values(tag_records(&["t1", "t2", "t3", "t4", "t5"]), |done, _| {
            if done == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .await?;

    assert!(outcome.cancelled);
    assert_eq!(outcome.inserted, 2);
    assert!(shell.get_record("t2").is_some());
    assert!(
        shell.get_record("t3").is_none(),
        "row 3 was never attempted"
    );
    Ok(())
}

/// The native path is held to a derived Dio's residual too: a record outside
/// it refuses the whole import before the master sees it, and one inside it
/// reaches the master (whose refusal here proves it got that far).
#[tokio::test]
async fn native_import_is_confined_to_the_residual() -> Result<()> {
    use vantage_vista::FilterOp;
    let shell =
        MockShell::new().with_capabilities(VistaCapabilities::new().with(Capability::Import));
    let not_lost = dio_over(&shell)
        .await?
        .with_query(&support::cond("status", FilterOp::Ne, "lost"))
        .await?;
    assert!(
        !not_lost.residual().conditions.is_empty(),
        "the master refused it"
    );

    let mut lost = tag_records(&["t1"]);
    lost["t1"].insert("status".to_string(), text("lost"));
    let err = not_lost
        .import_values(lost, |_, _| ControlFlow::Continue(()))
        .await
        .expect_err("outside the residual");
    assert!(err.is_conflict(), "refused before the master: {err}");

    let err = not_lost
        .import_values(tag_records(&["t2"]), |_, _| ControlFlow::Continue(()))
        .await
        .expect_err("the mock refuses a native import");
    assert!(
        err.is_unsupported(),
        "an in-set record reaches the master: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn advertised_native_import_is_taken_at_its_word() -> Result<()> {
    // A shell that *claims* Import but doesn't implement the op:
    // the Dio must route to the native path and surface the driver's
    // refusal — never silently fall back, which would hide a driver bug
    // behind 5,000 working inserts.
    let shell =
        MockShell::new().with_capabilities(VistaCapabilities::new().with(Capability::Import));
    let dio = dio_over(&shell).await?;

    let err = dio
        .import_values(tag_records(&["t1"]), |_, _| ControlFlow::Continue(()))
        .await
        .expect_err("the refusal surfaces");
    assert!(
        err.is_unsupported(),
        "the native path was attempted and refused: {err}"
    );
    Ok(())
}
