//! A write settled through A reaches B, derived from A for client 7, and B
//! judges it against its own set (use cases 1 to 5).

use std::sync::Arc;

use ciborium::Value as CborValue;
use tokio::sync::Semaphore;
use vantage_core::{Result, error};
use vantage_dataset::Inserted;
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_diorama::{
    Cache, CacheBackend, CacheStatus, CacheTable, ChangeFlash, Confirmation, FlashKind,
    FlashOutcome, Lens, MemoryCache, RecordStatus, RowStatus,
};
use vantage_types::Record;
use vantage_vista::{TableShell, Vista, forward_table_shell};

use crate::fixtures::{
    Pair, cached_cell, cell, client, events_until, holds, ids, invoice, invoice_master, patch,
    settle_events, until, view_showing,
};
use crate::support::{copy_master, memory_lens, record, text};

#[tokio::test]
async fn case1_a_row_moved_out_through_a_leaves_b() -> Result<()> {
    let Pair { a, b, .. } = Pair::eager().await;
    let b_view = view_showing(&b, &["x"]).await;

    a.flash(patch("x", &[("client_id", "8")])).await?;

    until("B's view drops x", async || ids(&b_view).is_empty()).await;
    assert!(!holds(&b, "x").await, "B's cache dropped x too");
    Ok(())
}

/// Two Dios over one cache table (they differ only in `live`) share the
/// bytes, so the fact only needs announcing; B's view filter judges it.
#[tokio::test]
async fn case1_shared_store_sibling_rechecks_its_filter() -> Result<()> {
    let lens = memory_lens(copy_master());
    let shell = invoice_master();
    let a = lens
        .dio(Vista::new("invoice", shell.clone()))
        .build()
        .await?;
    let b = lens
        .dio(Vista::new("invoice", shell))
        .live()
        .build()
        .await?;
    let b_view = b.table_scenery().filter(client("7")).open().await?;
    until("B's view shows x", async || ids(&b_view) == ["x"]).await;

    a.flash(patch("x", &[("client_id", "8")])).await?;

    until("B's view drops x", async || ids(&b_view).is_empty()).await;
    Ok(())
}

#[tokio::test]
async fn case2_read_by_id_after_the_move_is_not_found() -> Result<()> {
    let Pair { a, b, .. } = Pair::eager().await;
    let b_view = view_showing(&b, &["x"]).await;

    a.flash(patch("x", &[("client_id", "8")])).await?;
    until("B's view drops x", async || ids(&b_view).is_empty()).await;

    assert_eq!(b.cached().get_value("x").await?, None);
    let record = b.record_scenery("x").load_on_miss().open().await?;
    until("B's record scenery finds no x", async || {
        record.status() == RecordStatus::NotFound
    })
    .await;
    Ok(())
}

#[tokio::test]
async fn case3_a_change_inside_the_scope_repaints_b() -> Result<()> {
    let Pair { a, b, .. } = Pair::eager().await;
    let b_view = view_showing(&b, &["x"]).await;

    a.flash(patch("x", &[("amount", "11")])).await?;

    until("B repaints x", async || {
        cell(&b_view, "x", "amount").as_deref() == Some("11")
    })
    .await;
    Ok(())
}

#[tokio::test]
async fn case4_an_insert_through_a_matching_b_appears_in_b() -> Result<()> {
    let Pair { a, b, .. } = Pair::eager().await;
    let b_view = view_showing(&b, &["x"]).await;

    a.flash(ChangeFlash::insert(
        "y",
        record(&[("id", "y"), ("client_id", "7"), ("amount", "3")]),
    ))
    .await?;
    until("B shows y", async || {
        ids(&b_view).contains(&"y".to_string())
    })
    .await;

    let outcome = a
        .flash(ChangeFlash::insert_new(record(&[
            ("client_id", "7"),
            ("amount", "4"),
        ])))
        .await?;
    let settled = outcome.id.expect("the backend's id");
    until("B holds the backend's id", async || {
        holds(&b, &settled).await
    })
    .await;
    Ok(())
}

#[tokio::test]
async fn case4_an_insert_outside_b_stays_out() -> Result<()> {
    let Pair { a, b, .. } = Pair::eager().await;
    let b_view = view_showing(&b, &["x"]).await;
    let mut b_bus = b.subscribe_events();

    a.flash(ChangeFlash::insert(
        "w",
        record(&[("id", "w"), ("client_id", "8")]),
    ))
    .await?;
    // The sentinel: a later write inside B's set reaches B after w's would.
    a.flash(patch("x", &[("amount", "11")])).await?;

    assert_eq!(events_until(&mut b_bus, "changed x").await, ["changed x"]);
    assert_eq!(ids(&b_view), ["x"]);
    assert!(!holds(&b, "w").await);
    Ok(())
}

#[tokio::test]
async fn case5_a_delete_through_a_leaves_b() -> Result<()> {
    let Pair { a, b, .. } = Pair::eager().await;
    let b_view = view_showing(&b, &["x"]).await;

    a.flash(ChangeFlash::delete("x")).await?;

    until("B's view drops x", async || ids(&b_view).is_empty()).await;
    assert!(!holds(&b, "x").await);
    Ok(())
}

#[tokio::test]
async fn a_clear_through_a_empties_b() -> Result<()> {
    let Pair { a, b, .. } = Pair::eager().await;
    let b_view = view_showing(&b, &["x"]).await;

    a.flash(ChangeFlash::clear()).await?;

    until("B's view empties", async || ids(&b_view).is_empty()).await;
    assert!(!holds(&b, "x").await);
    Ok(())
}

/// The loop-breakers: B applies A's fact without publishing it back, and A
/// skips its own, so each bus hears the write exactly once.
#[tokio::test]
async fn one_write_raises_one_settle_event_per_bus() -> Result<()> {
    let Pair { a, b, .. } = Pair::eager().await;
    let mut a_bus = a.subscribe_events();
    let mut b_bus = b.subscribe_events();

    a.flash(patch("x", &[("amount", "11")])).await?;
    // Outside B's set: it must raise nothing on B.
    a.flash(patch("z", &[("amount", "6")])).await?;
    // The sentinel, a third write that B hears: once it is in, so is z's.
    a.flash(patch("x", &[("note", "sentinel")])).await?;
    let b_seen = events_until(&mut b_bus, "changed x").await;
    let b_seen = [b_seen, events_until(&mut b_bus, "changed x").await].concat();

    assert_eq!(
        settle_events(&mut a_bus),
        [
            "pending x",
            "changed x",
            "pending z",
            "changed z",
            "pending x",
            "changed x"
        ]
    );
    assert_eq!(b_seen, ["changed x", "changed x"]);
    assert!(settle_events(&mut b_bus).is_empty());
    Ok(())
}

/// A failed write rolls back on A and publishes nothing: B never saw the
/// staged value, so it stays on the confirmed row.
#[tokio::test]
async fn a_failed_write_through_a_leaves_b_on_the_confirmed_row() -> Result<()> {
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(copy_master())
        .writer(|cx, flash| async move {
            match flash.patch().get("amount") {
                Some(_) => Err(error!("the backend refused the amount")),
                None => {
                    let id = flash.id().expect("a patch has an id").to_string();
                    cx.master().patch_value(id, flash.patch()).await?;
                    Ok(FlashOutcome::applied())
                }
            }
        })
        .build()?;
    let Pair { a, b, .. } = Pair::on(&lens, invoice_master()).await;
    let b_view = view_showing(&b, &["x"]).await;
    let mut b_bus = b.subscribe_events();

    assert!(a.flash(patch("x", &[("amount", "99")])).await.is_err());
    // The sentinel: a write the backend takes, after the refused one.
    a.flash(patch("x", &[("note", "kept")])).await?;

    assert_eq!(events_until(&mut b_bus, "changed x").await, ["changed x"]);
    assert_eq!(cell(&b_view, "x", "amount").as_deref(), Some("10"));
    Ok(())
}

/// Two flashes on `x` stack. The first is confirmed while the second is
/// staged over it, so its value isn't published then. The second fails and
/// restores the first's value, and B re-reads `x` and gets it.
#[tokio::test]
async fn a_rollback_onto_a_confirmed_flash_reaches_b() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let writer_gate = gate.clone();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(copy_master())
        .writer(move |cx, flash| {
            let gate = writer_gate.clone();
            async move {
                if flash.patch().get("amount") == Some(&text("99")) {
                    return Err(error!("the backend refused the amount"));
                }
                gate.acquire().await.expect("gate open").forget();
                let id = flash.id().expect("a patch has an id").to_string();
                cx.master().patch_value(id, flash.patch()).await?;
                Ok(FlashOutcome::applied())
            }
        })
        .build()?;
    let Pair { a, b, .. } = Pair::on(&lens, invoice_master()).await;
    let b_view = view_showing(&b, &["x"]).await;
    let mut a_bus = a.subscribe_events();

    // The first stages and queues, its write-through held at the gate; the
    // second stages over it and queues behind it.
    let first = tokio::spawn({
        let a = a.clone();
        async move { a.flash(patch("x", &[("amount", "11")])).await }
    });
    events_until(&mut a_bus, "pending x").await;
    let second = tokio::spawn({
        let a = a.clone();
        async move { a.flash(patch("x", &[("amount", "99")])).await }
    });
    events_until(&mut a_bus, "pending x").await;
    gate.add_permits(1);
    first.await.expect("first flash task")?;
    assert!(second.await.expect("second flash task").is_err());

    until("B shows the confirmed 11", async || {
        cell(&b_view, "x", "amount").as_deref() == Some("11")
    })
    .await;
    Ok(())
}

/// B's flash on `x` is in flight when A's change to `x` arrives, so B skips
/// it rather than race its own flash. Once B's flash settles, B re-reads
/// `x` and has both changes. B's settle asks A to re-read too, since B's
/// row was stale outside its own field.
#[tokio::test]
async fn a_fact_skipped_for_a_pending_flash_is_rechecked_once_it_settles() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let writer_gate = gate.clone();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(copy_master())
        .writer(move |cx, flash| {
            let gate = writer_gate.clone();
            async move {
                if flash.patch().contains_key("note") {
                    gate.acquire().await.expect("gate open").forget();
                }
                let id = flash.id().expect("a patch or insert has an id").to_string();
                if flash.kind() == &FlashKind::Insert {
                    cx.master().insert_value(id, flash.patch()).await?;
                } else {
                    cx.master().patch_value(id, flash.patch()).await?;
                }
                Ok(FlashOutcome::applied())
            }
        })
        .build()?;
    let Pair { a, b, .. } = Pair::on(&lens, invoice_master()).await;
    view_showing(&b, &["x"]).await;
    let mut b_bus = b.subscribe_events();

    let b_edit = tokio::spawn({
        let b = b.clone();
        async move { b.flash(patch("x", &[("note", "b")])).await }
    });
    events_until(&mut b_bus, "pending x").await;
    a.flash(patch("x", &[("amount", "11")])).await?;
    // The sentinel: B has handled A's change to x once it hears of y.
    a.flash(ChangeFlash::insert(
        "y",
        record(&[("id", "y"), ("client_id", "7")]),
    ))
    .await?;
    events_until(&mut b_bus, "inserted y").await;
    gate.add_permits(1);
    b_edit.await.expect("B's flash task")?;

    until("B has A's amount and its own note", async || {
        cached_cell(&b, "x", "amount").await.as_deref() == Some("11")
            && cached_cell(&b, "x", "note").await.as_deref() == Some("b")
    })
    .await;
    until("A has B's note and keeps its amount", async || {
        cached_cell(&a, "x", "amount").await.as_deref() == Some("11")
            && cached_cell(&a, "x", "note").await.as_deref() == Some("b")
    })
    .await;
    Ok(())
}

/// A backend that keeps a new row under an id of its own: B holds the row
/// under the settled id, never the one A minted.
#[tokio::test]
async fn a_rekeyed_insert_reaches_b_under_the_settled_id() -> Result<()> {
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(copy_master())
        .writer(|_cx, _flash| async { Ok(FlashOutcome::applied().with_id("x2")) })
        .build()?;
    let Pair { a, b, .. } = Pair::on(&lens, invoice_master()).await;
    let b_view = view_showing(&b, &["x"]).await;

    a.flash(ChangeFlash::insert(
        "tmp",
        record(&[("id", "tmp"), ("client_id", "7")]),
    ))
    .await?;

    until("B shows x2", async || {
        ids(&b_view).contains(&"x2".to_string())
    })
    .await;
    assert!(holds(&b, "x2").await);
    assert!(!holds(&b, "tmp").await);
    Ok(())
}

/// The master already holds `x`, at an amount neither Dio has read yet. An
/// insert of `x` through A writes nothing there: A caches the row the master
/// reports, not the flash's, and B gets that row too.
#[tokio::test]
async fn existed_insert_caches_the_stored_row_and_publishes_it() -> Result<()> {
    let Pair { a, b, shell } = Pair::eager().await;
    let b_view = view_showing(&b, &["x"]).await;
    shell.set_record("x", invoice("x", "7", "12"));

    let mut typed = invoice("x", "7", "99");
    typed.insert("note".into(), text("typed"));
    let outcome = a.flash(ChangeFlash::insert("x", typed)).await?;
    assert_eq!(outcome.confirmation, Confirmation::Existed);
    assert_eq!(cached_cell(&a, "x", "amount").await.as_deref(), Some("12"));
    assert_eq!(cached_cell(&a, "x", "note").await, None);

    until("B shows the stored amount", async || {
        cell(&b_view, "x", "amount").as_deref() == Some("12")
    })
    .await;
    assert_eq!(cached_cell(&b, "x", "note").await, None);
    Ok(())
}

/// A master whose inserts are taken to be stored later: nothing is stored
/// now, and the answer is `Accepted`.
#[derive(Clone)]
struct AcceptsInserts {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(AcceptsInserts, inner, {
    async fn insert(
        &self,
        id: Option<&str>,
        _record: &Record<CborValue>,
    ) -> Result<(String, Inserted<CborValue>)> {
        let id = id.expect("the tests insert with an id");
        Ok((id.to_string(), Inserted::Accepted))
    }
});

/// The status each cache table holds `id` under, for the tables that hold it.
async fn statuses(backend: &MemoryCache, id: &str) -> Result<Vec<CacheStatus>> {
    let mut found = Vec::new();
    for name in backend.list_tables().await? {
        let table: Arc<dyn CacheTable> = backend.open_table(&name).await?;
        if let Some((_, status)) = table.get_value_with_status(id).await? {
            found.push(status);
        }
    }
    Ok(found)
}

/// An accepted insert shows in A, unconfirmed, and B never hears of it. A
/// read of the master that lacks the row drops it.
#[tokio::test]
async fn accepted_insert_stays_unconfirmed_until_read() -> Result<()> {
    let backend = Arc::new(MemoryCache::new());
    let lens = Lens::builder()
        .cache(Cache::backend(backend.clone()))
        .loader(copy_master())
        .build()?;
    let master =
        Vista::new("invoice", invoice_master()).map_shell(|inner| AcceptsInserts { inner });
    let a = lens.dio(master).build().await?;
    let b = a.derive().with_condition(client("7")).build().await?;
    let _b_view = view_showing(&b, &["x"]).await;
    let a_view = view_showing(&a, &["x", "z"]).await;
    let mut b_bus = b.subscribe_events();

    let outcome = a
        .flash(ChangeFlash::insert("y", invoice("y", "7", "3")))
        .await?;
    assert_eq!(outcome.confirmation, Confirmation::Accepted);
    assert_eq!(cached_cell(&a, "y", "amount").await.as_deref(), Some("3"));
    assert_eq!(statuses(&backend, "y").await?, [CacheStatus::Unconfirmed]);
    // A's own view shows the row, marked queued.
    until("A's view shows y unconfirmed", async || {
        a_view
            .row_by_id("y")
            .is_some_and(|row| row.status == RowStatus::Unconfirmed)
    })
    .await;
    let y = a.record_scenery("y").open().await?;
    assert_eq!(y.status(), RecordStatus::Unconfirmed);

    // The master doesn't hold y, so a patch of it fails; the rollback puts
    // the accepted row back as it was, still unconfirmed.
    assert!(a.flash(patch("y", &[("amount", "4")])).await.is_err());
    assert_eq!(cached_cell(&a, "y", "amount").await.as_deref(), Some("3"));
    assert_eq!(statuses(&backend, "y").await?, [CacheStatus::Unconfirmed]);

    // The sentinel: a write B hears, published after y would have been.
    a.flash(patch("x", &[("amount", "11")])).await?;
    assert_eq!(events_until(&mut b_bus, "changed x").await, ["changed x"]);
    assert!(!holds(&b, "y").await);

    a.refresh().await?;
    assert!(statuses(&backend, "y").await?.is_empty());
    assert!(!holds(&a, "y").await);
    Ok(())
}
