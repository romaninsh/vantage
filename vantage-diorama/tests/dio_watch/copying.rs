//! A parent change made while the child's eager start copies the set is
//! applied over the copy, detached or blocking.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tokio::sync::{Notify, Semaphore};
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, DioEvent, Lens, Loader};

use super::{Parent, in_transit, master, note_of, noted, wait_until};

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
