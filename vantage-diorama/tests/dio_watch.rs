//! A Dio built over another Dio's narrowed facade follows the parent live:
//! `.live()` subscribes to the parent's event bus through the facade, and a
//! parent change that moves a row across the filter shows up downstream.
//!
//! The follow recovers: a child that lags behind its parent re-reads the set,
//! and a change made while the child's eager start copies the set is applied
//! over the copy.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use futures::StreamExt;
use tokio::sync::{Notify, Semaphore, broadcast};
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, ChangeEvent, Dio, DioEvent, Lens, Loader};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, FilterOp, TableShell, Vista, VistaCapabilities, VistaChange,
    VistaChangeStream, VistaMetadata, forward_table_shell, mocks::MockShell,
};

fn shipment(status: &str) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("status".to_string(), CborValue::Text(status.to_string()));
    record
}

fn noted(status: &str, note: &str) -> Record<CborValue> {
    let mut record = shipment(status);
    record.insert("note".to_string(), CborValue::Text(note.to_string()));
    record
}

fn master() -> MockShell {
    MockShell::new().with_metadata(
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("status", "String"))
            .with_column(Column::new("note", "String"))
            .with_id_column("id"),
    )
}

/// An eager loader that hands the whole master to `reconcile`.
fn copy_master() -> Loader {
    Loader::eager(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    })
}

/// An eager Dio over `master`, following it live when `live` is set.
async fn eager_dio(master: Vista, live: bool) -> Result<Dio> {
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(copy_master())
        .build()?;
    let builder = lens.dio(master);
    if live {
        builder.live().build().await
    } else {
        builder.build().await
    }
}

/// The parent's facade narrowed to the shipments in transit.
fn in_transit(parent: &Dio) -> Result<Vista> {
    parent
        .cached()
        .with_condition("status", FilterOp::Eq, "In transit")
}

/// A parent Dio and the shell behind its master.
struct Parent {
    dio: Dio,
    shell: MockShell,
}

impl Parent {
    async fn over(shell: MockShell) -> Result<Self> {
        let dio = eager_dio(Vista::new("shipment", shell.clone()), false).await?;
        Ok(Self { dio, shell })
    }

    /// An upstream change: the master stores `record`, and the change is
    /// pushed into the parent.
    async fn upsert(&self, id: &str, record: Record<CborValue>) -> Result<()> {
        self.shell.set_record(id, record.clone());
        let new = Some(record);
        self.dio
            .apply_change(ChangeEvent::Updated { id: id.into(), new })
            .await
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.shell.remove_record(id);
        self.dio
            .apply_change(ChangeEvent::Deleted { id: id.into() })
            .await
    }
}

/// Wait until `pred` holds for `dio`, re-checking on every event its bus
/// publishes. The timeout bounds a hang; it is not a speed assertion.
async fn wait_until<F>(dio: &Dio, mut bus: broadcast::Receiver<DioEvent>, pred: F)
where
    F: AsyncFn(&Dio) -> bool,
{
    let wait = async {
        while !pred(dio).await {
            let _ = bus.recv().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .expect("the follow never caught up");
}

async fn note_of(dio: &Dio, id: &str) -> Option<CborValue> {
    let row = dio.cached().get_value(id).await.ok().flatten();
    row.and_then(|r| r.get("note").cloned())
}

async fn holds(dio: &Dio, id: &str) -> bool {
    dio.cached().get_value(id).await.ok().flatten().is_some()
}

#[tokio::test]
async fn a_narrowed_dio_follows_its_parent_across_the_filter() -> Result<()> {
    let shell = master();
    shell.set_record("s1", shipment("In transit"));
    shell.set_record("s2", shipment("Delivered"));
    let parent = Parent::over(shell).await?;
    let derived = eager_dio(in_transit(&parent.dio)?, true).await?;

    assert!(derived.master().capabilities().has(Capability::Subscribe));
    assert!(holds(&derived, "s1").await);
    assert!(!holds(&derived, "s2").await);

    // Patched into the filter: appears downstream with the new value.
    let bus = derived.subscribe_events();
    parent.upsert("s2", shipment("In transit")).await?;
    wait_until(&derived, bus, async |d| holds(d, "s2").await).await;

    // Patched out of the filter: removed downstream.
    let bus = derived.subscribe_events();
    parent.upsert("s1", shipment("Delivered")).await?;
    wait_until(&derived, bus, async |d| !holds(d, "s1").await).await;

    // Removed from the parent: removed downstream.
    let bus = derived.subscribe_events();
    parent.delete("s2").await?;
    wait_until(&derived, bus, async |d| !holds(d, "s2").await).await;
    Ok(())
}

/// `live()` asks to follow the master "when it can": over a master without
/// `Subscribe` the build succeeds and the Dio simply doesn't follow.
#[tokio::test]
async fn live_over_a_master_without_subscribe_builds() -> Result<()> {
    let shell = master();
    shell.set_record("s1", shipment("In transit"));
    let master = Vista::new("shipment", shell);
    assert!(!master.capabilities().has(Capability::Subscribe));
    let dio = eager_dio(master, true).await?;
    assert!(holds(&dio, "s1").await);
    Ok(())
}

/// A narrowed child whose follow falls more than a bus's worth of events
/// behind its parent lags, re-reads the set, and catches up.
///
/// The parent's changes are applied without yielding, so on this
/// single-threaded runtime the child's follow can't read any of them until
/// all are published: its subscription to the parent's bus overflows.
#[tokio::test]
async fn a_lagged_narrowed_child_catches_up() -> Result<()> {
    const ROWS: usize = 200;
    let parent = Parent::over(master()).await?;
    let child = eager_dio(in_transit(&parent.dio)?, true).await?;
    let bus = child.subscribe_events();
    // The recovery goes through a re-read, which announces itself first.
    let mut refreshes = child.subscribe_events();
    let reread = tokio::spawn(async move {
        loop {
            match refreshes.recv().await {
                Ok(DioEvent::Refreshing) => return true,
                Err(broadcast::error::RecvError::Closed) => return false,
                _ => {}
            }
        }
    });

    for i in 0..ROWS {
        let status = if i % 2 == 0 {
            "In transit"
        } else {
            "Delivered"
        };
        parent.upsert(&format!("s{i:03}"), shipment(status)).await?;
    }

    let caught_up = async |d: &Dio| d.cached().count().await.ok() == Some((ROWS / 2) as i64);
    wait_until(&child, bus, caught_up).await;
    let reread = tokio::time::timeout(Duration::from_secs(5), reread).await;
    assert!(
        reread.expect("no re-read was announced").expect("listener"),
        "the child recovered through a re-read"
    );
    Ok(())
}

/// A watchable master whose stream yields one `Invalidated`, then stays open.
#[derive(Clone)]
struct InvalidatingShell {
    inner: Box<dyn TableShell>,
    capabilities: VistaCapabilities,
}

forward_table_shell!(InvalidatingShell, inner, {
    async fn watch(&self) -> Result<VistaChangeStream> {
        let invalidated = futures::stream::once(async { Ok(VistaChange::Invalidated) });
        Ok(Box::pin(invalidated.chain(futures::stream::pending())))
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
});

/// The master invalidates the set and can't be read: the follow's re-read
/// fails, and the bus says so as a failed refresh, not as a failed range load.
#[tokio::test]
async fn a_failed_recovery_reread_publishes_refresh_failed() -> Result<()> {
    let shell = master();
    shell.set_fail_reads(true);
    let capabilities = shell.capabilities().with(Capability::Subscribe);
    let vista = Vista::new("shipment", shell)
        .map_shell(|inner| InvalidatingShell { inner, capabilities });
    let lens = Lens::builder().cache(Cache::memory()).build()?;
    let dio = lens.dio(vista).live().build().await?;
    let mut bus = dio.subscribe_events();

    let seen = tokio::time::timeout(Duration::from_secs(3), async {
        let mut seen = Vec::new();
        loop {
            match bus.recv().await {
                Ok(DioEvent::RefreshFailed { error }) => return (error, seen),
                Ok(event) => seen.push(event),
                Err(e) => panic!("bus: {e}"),
            }
        }
    })
    .await;
    let (error, before) = seen.expect("the failed re-read is published");
    assert!(error.contains("injected"), "{error}");
    let stand_in =
        |e: &DioEvent| matches!(e, DioEvent::LoadFailed { range, .. } if range.is_empty());
    assert!(!before.iter().any(stand_in), "{before:?}");
    Ok(())
}

/// A child whose eager start waits on `gate` after taking its snapshot of
/// the master, and signals `snapshot` once it has one.
fn gated_copy(snapshot: Arc<Notify>, gate: Arc<Semaphore>, blocking: bool) -> Result<Arc<Lens>> {
    let loader = Loader::eager(move |cx| {
        let (snapshot, gate) = (snapshot.clone(), gate.clone());
        async move {
            let rows = cx.master().list_values().await?;
            snapshot.notify_one();
            gate.acquire().await.expect("gate open").forget();
            cx.reconcile(rows).await
        }
    });
    Lens::builder()
        .cache(Cache::memory())
        .loader(loader.blocking(blocking))
        .build()
}

/// A parent holding `s1` in transit, noted `before`.
async fn noted_parent() -> Result<Parent> {
    let shell = master();
    shell.set_record("s1", noted("In transit", "before"));
    Parent::over(shell).await
}

/// A change made while a detached eager start copies the set is applied
/// after the copy lands, not overwritten by the older snapshot.
#[tokio::test]
async fn a_change_during_a_detached_copy_is_applied_over_it() -> Result<()> {
    let parent = noted_parent().await?;
    let (snapshot, gate) = (Arc::new(Notify::new()), Arc::new(Semaphore::new(0)));
    let lens = gated_copy(snapshot.clone(), gate.clone(), false)?;
    let child = lens.dio(in_transit(&parent.dio)?).live().build().await?;
    snapshot.notified().await;

    parent.upsert("s1", noted("In transit", "after")).await?;
    let mut bus = child.subscribe_events();
    gate.add_permits(1);
    let seeded = async { while !matches!(bus.recv().await, Ok(DioEvent::Seeded)) {} };
    tokio::time::timeout(Duration::from_secs(5), seeded)
        .await
        .expect("the copy never landed");

    let after = Some(CborValue::Text("after".into()));
    wait_until(&child, bus, async |d| note_of(d, "s1").await == after).await;
    Ok(())
}

/// A change made while a blocking eager start copies the set is applied
/// once the build returns, not lost.
#[tokio::test]
async fn a_change_during_a_blocking_copy_is_not_lost() -> Result<()> {
    let parent = noted_parent().await?;
    let (snapshot, gate) = (Arc::new(Notify::new()), Arc::new(Semaphore::new(0)));
    let lens = gated_copy(snapshot.clone(), gate.clone(), true)?;
    let master = in_transit(&parent.dio)?;
    let building = tokio::spawn(async move { lens.dio(master).live().build().await });
    snapshot.notified().await;

    parent.upsert("s1", noted("In transit", "after")).await?;
    gate.add_permits(1);
    let child = building.await.expect("build task")?;
    let bus = child.subscribe_events();

    let after = Some(CborValue::Text("after".into()));
    wait_until(&child, bus, async |d| note_of(d, "s1").await == after).await;
    Ok(())
}
