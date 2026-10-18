//! Where the table hub stops: a Dio alone, a Dio over another Dio's facade,
//! another Lens (use case 8), another hub key (which also keys the cache
//! table), a follower that falls behind, and a Dio that drops.

use std::sync::Arc;

use tokio::sync::{Semaphore, broadcast};
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{ChangeEvent, ChangeFlash, DioEvent};
use vantage_vista::mocks::MockShell;
use vantage_vista::{FilterOp, Vista};

use crate::fixtures::{
    Pair, WITHIN, cached_cell, client, events_until, holds, invoice, invoice_master, patch,
    settle_events, until,
};
use crate::support::{copy_master, eager_dio, memory_lens, meta, record};

/// With no other Dio over the table, a bus sees what it always has.
#[tokio::test]
async fn a_lone_dio_sees_only_its_own_settle_event() -> Result<()> {
    let dio = eager_dio(Vista::new("invoice", invoice_master())).await;
    let mut bus = dio.subscribe_events();

    dio.flash(patch("x", &[("amount", "11")])).await?;

    assert_eq!(settle_events(&mut bus), ["pending x", "changed x"]);
    Ok(())
}

/// A Dio over A's facade follows A's bus and joins no hub, so a write
/// through A reaches it once however many Dios A shares the table with.
#[tokio::test]
async fn a_dio_over_a_facade_does_not_hear_its_parent_twice() -> Result<()> {
    async fn heard_by_facade_child(with_sibling: bool) -> Result<Vec<String>> {
        let lens = memory_lens(copy_master());
        let a = lens
            .dio(Vista::new("invoice", invoice_master()))
            .build()
            .await?;
        let _sibling = match with_sibling {
            true => Some(a.derive().with_condition(client("7")).build().await?),
            false => None,
        };
        let facade = a.cached().with_condition("client_id", FilterOp::Eq, "7")?;
        let child = lens.dio(facade).live().build().await?;
        until("the child holds x", async || holds(&child, "x").await).await;
        let mut bus = child.subscribe_events();

        a.flash(patch("x", &[("amount", "11")])).await?;
        // The sentinel: a second write, heard after everything the first
        // one raised.
        a.flash(patch("x", &[("note", "sentinel")])).await?;
        until("the child repaints x", async || {
            cached_cell(&child, "x", "note").await.as_deref() == Some("sentinel")
        })
        .await;
        Ok(settle_events(&mut bus))
    }

    let alone = heard_by_facade_child(false).await?;
    assert!(alone.contains(&"changed x".to_string()), "{alone:?}");
    assert_eq!(heard_by_facade_child(true).await?, alone);
    Ok(())
}

/// The hub's scope is the Lens: Dios on another Lens hear nothing.
#[tokio::test]
async fn case8_separate_lenses_share_nothing() -> Result<()> {
    let shell = invoice_master();
    let a = memory_lens(copy_master())
        .dio(Vista::new("invoice", shell.clone()))
        .build()
        .await?;
    let other = Pair::on(&memory_lens(copy_master()), shell).await;
    let mut b_bus = other.b.subscribe_events();

    a.flash(patch("x", &[("client_id", "8")])).await?;
    // The sentinel: a write on B's own Lens, which B hears.
    other.a.flash(sentinel_insert()).await?;

    assert_eq!(events_until(&mut b_bus, "inserted y").await, ["inserted y"]);
    assert_eq!(
        cached_cell(&other.b, "x", "client_id").await.as_deref(),
        Some("7")
    );
    Ok(())
}

/// Two Dios given different hub keys never exchange facts, even over the
/// same table and Lens.
#[tokio::test]
async fn dios_with_different_hub_keys_exchange_nothing() -> Result<()> {
    let lens = memory_lens(copy_master());
    let shell = invoice_master();
    let narrowed =
        || Vista::new("invoice", shell.clone()).with_condition("client_id", FilterOp::Eq, "7");
    let a = lens
        .dio(Vista::new("invoice", shell.clone()))
        .hub("left")
        .build()
        .await?;
    let b = lens.dio(narrowed()?).hub("right").build().await?;
    let c = lens
        .dio(Vista::new("invoice", shell.clone()))
        .hub("right")
        .build()
        .await?;
    let mut b_bus = b.subscribe_events();

    a.flash(patch("x", &[("client_id", "8")])).await?;
    // The sentinel: C shares B's hub, and B hears C.
    c.flash(sentinel_insert()).await?;

    let seen = events_until(&mut b_bus, "inserted y").await;
    assert_eq!(seen, ["inserted y"], "B heard C, and nothing of A's");
    assert!(holds(&b, "x").await);
    Ok(())
}

/// Two sessions whose vistas have the same name and recorded query, keyed
/// apart: each session's master holds different rows (a rule the vista
/// doesn't record), so the key keeps their cache tables apart too.
#[tokio::test]
async fn differently_keyed_dios_never_share_a_cache_table() -> Result<()> {
    let lens = memory_lens(copy_master());
    let mine = MockShell::new()
        .with_metadata(meta(&["id", "client_id", "amount", "note"]))
        .with_record("x", invoice("x", "7", "10"));
    let theirs = MockShell::new()
        .with_metadata(meta(&["id", "client_id", "amount", "note"]))
        .with_record("z", invoice("z", "8", "5"));
    let session_1 = lens
        .dio(Vista::new("invoice", mine))
        .hub("s1")
        .build()
        .await?;
    let session_2 = lens
        .dio(Vista::new("invoice", theirs))
        .hub("s2")
        .build()
        .await?;

    let keys = |rows: indexmap::IndexMap<String, _>| rows.into_keys().collect::<Vec<_>>();
    assert_eq!(keys(session_1.cached().list_values().await?), ["x"]);
    assert_eq!(keys(session_2.cached().list_values().await?), ["z"]);
    Ok(())
}

/// A row for client 7 that every B hears about, once its hub carries it.
fn sentinel_insert() -> ChangeFlash {
    ChangeFlash::insert("y", record(&[("id", "y"), ("client_id", "7")]))
}

/// A layer that renames each vista (one name per session) passes the table
/// as the hub key, and its Dios hear each other under any name.
#[tokio::test]
async fn differently_named_vistas_over_one_table_share_a_hub_by_key() -> Result<()> {
    let lens = memory_lens(copy_master());
    let shell = invoice_master();
    let a = lens
        .dio(Vista::new("invoice", shell.clone()))
        .hub("invoice")
        .build()
        .await?;
    let b = lens
        .dio(Vista::new("session-1/invoice", shell).with_condition(
            "client_id",
            FilterOp::Eq,
            "7",
        )?)
        .hub("invoice")
        .build()
        .await?;

    a.flash(patch("x", &[("amount", "11")])).await?;

    until("B repaints x", async || {
        cached_cell(&b, "x", "amount").await.as_deref() == Some("11")
    })
    .await;
    Ok(())
}

/// A follower that falls more than the hub's capacity behind re-reads its
/// set and ends consistent with the table.
///
/// B's master holds every `get` at a gate. A notify-only change makes B
/// re-read `x`, so B's follower waits at the gate while A publishes more
/// changes than the hub holds.
#[tokio::test]
async fn a_lagging_follower_rereads_its_set() -> Result<()> {
    const CHANGES: usize = 400;
    let lens = memory_lens(copy_master());
    let shell = invoice_master();
    let gate = Arc::new(Semaphore::new(0));
    let a = lens
        .dio(Vista::new("invoice", shell.clone()))
        .build()
        .await?;
    let gated = Vista::new("invoice", shell.clone().gated(gate.clone()));
    let b = lens
        .dio(gated.with_condition("client_id", FilterOp::Eq, "7")?)
        .build()
        .await?;
    let mut refreshes = b.subscribe_events();
    let reread = tokio::spawn(async move {
        loop {
            match refreshes.recv().await {
                Ok(DioEvent::Refreshing) => return true,
                Err(broadcast::error::RecvError::Closed) => return false,
                _ => {}
            }
        }
    });

    a.apply_change(ChangeEvent::Updated {
        id: "x".into(),
        new: None,
    })
    .await?;
    until("B waits at the gate", async || shell.get_log() == ["x"]).await;
    for i in 0..CHANGES {
        let id = format!("r{:02}", i % 50);
        let row = invoice(&id, if i % 2 == 0 { "7" } else { "8" }, "1");
        shell.set_record(id.clone(), row.clone());
        a.apply_change(ChangeEvent::Updated { id, new: Some(row) })
            .await?;
    }
    gate.add_permits(CHANGES);

    let in_seven = shell
        .record_ids()
        .into_iter()
        .filter(|id| {
            shell
                .get_record(id)
                .and_then(|row| row.get("client_id").cloned())
                == Some(crate::support::text("7"))
        })
        .count() as i64;
    until("B matches the table", async || {
        b.cached().count().await.ok() == Some(in_seven)
    })
    .await;
    let reread = tokio::time::timeout(WITHIN, reread).await;
    assert!(
        reread.expect("no re-read was announced").expect("listener"),
        "B recovered through a re-read"
    );
    Ok(())
}

#[tokio::test]
async fn the_hub_does_not_keep_a_dio_alive() -> Result<()> {
    let Pair { a, b, .. } = Pair::eager().await;
    let weak = b.downgrade();
    drop(b);

    until("B is gone", async || weak.upgrade().is_none()).await;
    a.flash(patch("x", &[("client_id", "8")])).await?;
    Ok(())
}

/// Two vistas narrowed apart through `with_condition` get a cache table
/// each. A rule the vista never sees would not, which is why a session's
/// rule must also be a vista condition.
#[tokio::test]
async fn differing_applied_conditions_get_distinct_dios() -> Result<()> {
    let lens = memory_lens(copy_master());
    let shell = invoice_master();
    let narrowed = |id: &str| {
        Vista::new("invoice", shell.clone()).with_condition("client_id", FilterOp::Eq, id)
    };
    let seven = lens.dio(narrowed("7")?).build().await?;
    let eight = lens.dio(narrowed("8")?).build().await?;

    let keys = |rows: indexmap::IndexMap<String, _>| rows.into_keys().collect::<Vec<_>>();
    assert_eq!(keys(seven.cached().list_values().await?), ["x"]);
    assert_eq!(keys(eight.cached().list_values().await?), ["z"]);
    Ok(())
}
