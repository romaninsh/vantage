//! Effective write capabilities, and writes a writer routes away from a
//! read-only master.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, ChangeFlash, DioEvent, FlashKind, FlashOutcome, Lens, ServoStatus};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, Denials, TableShell, Vista, forbidden, forward_table_shell,
    mocks::MockShell,
};

use super::{memory_lens, metadata, read_only_shell, vista_over};
use crate::support::flash::{cached, seeded_dio};
use crate::support::{record, text};

#[tokio::test]
async fn write_capabilities_default_to_the_master() -> Result<()> {
    let lens = memory_lens();

    let read_only = lens
        .dio(vista_over(&read_only_shell(), "csv"))
        .build()
        .await?;
    let caps = read_only.write_capabilities();
    assert!(!caps.can_insert && !caps.can_update && !caps.can_delete);

    let writable = lens
        .dio(vista_over(&MockShell::new(), "db"))
        .build()
        .await?;
    let caps = writable.write_capabilities();
    assert!(caps.can_insert && caps.can_update && caps.can_delete);
    Ok(())
}

#[tokio::test]
async fn a_writer_makes_a_read_only_master_writable() -> Result<()> {
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer(|_cx, _flash| async move { Ok(FlashOutcome::applied()) })
        .build()?;
    let dio = lens
        .dio(vista_over(&read_only_shell(), "csv"))
        .build()
        .await?;

    let caps = dio.write_capabilities();
    assert!(
        caps.can_insert && caps.can_update && caps.can_delete,
        "the writer takes the writes; the master's own caps no longer gate editing"
    );
    Ok(())
}

/// A denial on the master holds under a Lens writer: the flash is refused
/// `Forbidden` before it is staged, and the writer never sees it.
#[tokio::test]
async fn writer_cannot_lift_a_denied_capability() -> Result<()> {
    let mut denials = Denials::new();
    denials.deny(Capability::Update, "policy/x.yaml#frozen");
    let calls: Arc<Mutex<Vec<FlashKind>>> = Arc::default();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer({
            let calls = calls.clone();
            move |_cx, flash| {
                calls.lock().unwrap().push(*flash.kind());
                async { Ok(FlashOutcome::applied()) }
            }
        })
        .build()?;
    let dio = lens
        .dio(Vista::new(
            "db",
            MockShell::new()
                .with_metadata(metadata())
                .with_denials(denials),
        ))
        .build()
        .await?;
    assert!(!dio.write_capabilities().can_update);

    let mut bus = dio.subscribe_events();
    let err = dio
        .flash(ChangeFlash::partial("1", record(&[("name", "Bea")])))
        .await
        .unwrap_err();
    assert!(err.is_forbidden(), "{err}");
    assert!(
        calls.lock().unwrap().is_empty(),
        "the writer is never called"
    );
    let events: Vec<DioEvent> = std::iter::from_fn(|| bus.try_recv().ok()).collect();
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, DioEvent::WritePending { .. })),
        "nothing was staged: {events:?}"
    );
    Ok(())
}

fn ssn_denied() -> Denials {
    let mut denials = Denials::new();
    denials.deny_column("ssn", "policy/x.yaml#pii");
    denials
}

/// A master holding `p1`, whose `ssn` is "123".
fn people(denials: Denials) -> MockShell {
    MockShell::new()
        .with_metadata(metadata().with_column(Column::new("ssn", "String")))
        .with_denials(denials)
        .with_record(
            "p1",
            record(&[("id", "p1"), ("name", "Ada"), ("ssn", "123")]),
        )
}

/// A whole-row write keeps a denied column's stored value: the cache never
/// shows the caller's value, and the master keeps its own.
#[tokio::test]
async fn replace_keeps_a_denied_column() -> Result<()> {
    let shell = people(ssn_denied());
    let lens = memory_lens();
    let dio = seeded_dio(&lens, Vista::new("db", shell.clone())).await?;

    dio.flash(ChangeFlash::replace(
        "p1",
        record(&[("id", "p1"), ("name", "Bea"), ("ssn", "x")]),
    ))
    .await?;

    let row = cached(&dio, "p1").await?.unwrap();
    assert_eq!(row.get("name"), Some(&text("Bea")));
    assert_eq!(row.get("ssn"), Some(&text("123")));
    let stored = shell.get_record("p1").unwrap();
    assert_eq!(stored.get("ssn"), Some(&text("123")));
    Ok(())
}

/// A Lens writer gets the denied column at its cached value, or not at all
/// when the row isn't cached.
#[tokio::test]
async fn writer_never_sees_a_denied_column() -> Result<()> {
    let written: Arc<Mutex<Vec<Record<CborValue>>>> = Arc::default();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer({
            let written = written.clone();
            move |_cx, flash| {
                written.lock().unwrap().push(flash.patch().clone());
                async { Ok(FlashOutcome::applied()) }
            }
        })
        .build()?;
    let dio = seeded_dio(&lens, Vista::new("db", people(ssn_denied()))).await?;

    dio.flash(ChangeFlash::replace(
        "p1",
        record(&[("id", "p1"), ("name", "Bea"), ("ssn", "x")]),
    ))
    .await?;
    dio.flash(ChangeFlash::upsert(
        "p2",
        record(&[("id", "p2"), ("name", "Cy"), ("ssn", "x")]),
    ))
    .await?;

    let written = written.lock().unwrap();
    assert_eq!(written[0].get("ssn"), Some(&text("123")));
    assert_eq!(written[1].get("ssn"), None);
    assert_eq!(written[1].get("name"), Some(&text("Cy")));
    Ok(())
}

/// An upsert may create the row, so a denied `Insert` refuses it before
/// anything is staged.
#[tokio::test]
async fn upsert_needs_insert() -> Result<()> {
    let mut denials = Denials::new();
    denials.deny(Capability::Insert, "policy/x.yaml#locked");
    let lens = memory_lens();
    let dio = seeded_dio(&lens, Vista::new("db", people(denials))).await?;

    let mut bus = dio.subscribe_events();
    let err = dio
        .flash(ChangeFlash::upsert(
            "p1",
            record(&[("id", "p1"), ("name", "Bea")]),
        ))
        .await
        .unwrap_err();
    assert!(err.is_forbidden(), "{err}");
    let events: Vec<DioEvent> = std::iter::from_fn(|| bus.try_recv().ok()).collect();
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, DioEvent::WritePending { .. })),
        "nothing was staged: {events:?}"
    );
    let row = cached(&dio, "p1").await?.unwrap();
    assert_eq!(row.get("name"), Some(&text("Ada")));
    Ok(())
}

/// A master whose shell refuses every patch, as a rule the Dio can't see
/// would.
#[derive(Clone)]
struct Refusing {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Refusing, inner, {
    async fn patch(&self, _id: &str, _partial: &Record<CborValue>) -> Result<Record<CborValue>> {
        forbidden(Capability::Update, "policy/x.yaml#locked", "db")
    }
});

/// The master's own refusal comes back `Forbidden`, not a plain write
/// failure.
#[tokio::test]
async fn a_master_refusal_stays_forbidden() -> Result<()> {
    let lens = memory_lens();
    let master = Vista::new("db", people(Denials::new())).map_shell(|inner| Refusing { inner });
    let dio = seeded_dio(&lens, master).await?;

    let err = dio
        .flash(ChangeFlash::partial("p1", record(&[("name", "Bea")])))
        .await
        .unwrap_err();
    assert!(err.is_forbidden(), "{err}");
    Ok(())
}

// ---- routed writes: read-only master, changes land elsewhere ----------------

#[tokio::test]
async fn routed_flash_lands_in_the_log_vista_and_never_touches_the_master() -> Result<()> {
    let master_shell = read_only_shell();
    let log_shell = MockShell::new();
    let log = Arc::new(vista_over(&log_shell, "audit-log"));

    let route_log = log.clone();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer(move |_cx, flash| {
            let log = route_log.clone();
            async move {
                // The routing ergonomic: bind the change to the log vista and
                // save the merged record there.
                flash.active_record(log.as_ref())?.save().await?;
                Ok(FlashOutcome::applied())
            }
        })
        .build()?;
    let dio = seeded_dio(&lens, vista_over(&master_shell, "csv")).await?;

    let servo = dio.servo("c1").await?;
    servo.set("status", text("won"));
    servo.flash().await?.expect("dirty servo flashes");
    assert!(matches!(servo.status(), ServoStatus::Tracking));

    // The change landed in the log vista — the full merged record.
    let landed = log.get_value("c1").await?.expect("routed row landed");
    assert_eq!(landed.get("status"), Some(&text("won")));
    assert_eq!(landed.get("name"), Some(&text("Ada")));

    // The cache keeps showing the edit (until a future reconcile says
    // otherwise) — the grid the user is looking at reflects their change.
    let row = cached(&dio, "c1").await?.unwrap();
    assert_eq!(row.get("status"), Some(&text("won")));

    // The read-only master was never written.
    let master_row = dio.master().get_value("c1").await?.unwrap();
    assert_eq!(master_row.get("status"), Some(&text("lead")));
    Ok(())
}
