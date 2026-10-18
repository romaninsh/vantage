//! A live follow's pushed changes reach every Dio over the table, and one
//! subscription serves them all (use case 6).

use vantage_core::Result;
use vantage_diorama::{Cache, Dio, DioEvent, Lens};
use vantage_vista::{FilterOp, Vista, VistaChange};

use crate::fixtures::{WITHIN, cached_cell, client, holds, invoice, live_master, until, updated};
use crate::support::{copy_master, memory_lens};

async fn narrowed_live(feed_dio: &Dio, id: &str) -> Dio {
    feed_dio
        .derive()
        .with_condition(client(id))
        .build()
        .await
        .expect("derived Dio builds")
}

/// A live Dio over the whole table: the first one becomes its feed.
async fn feed_on(lens: &std::sync::Arc<Lens>, vista: Vista) -> Dio {
    lens.dio(vista).live().build().await.expect("feed builds")
}

#[tokio::test]
async fn case6_a_live_feed_on_a_reaches_a_non_live_b() -> Result<()> {
    let lens = memory_lens(copy_master());
    let (vista, feed) = live_master();
    let a = feed_on(&lens, vista.clone()).await;
    let b = lens
        .dio(vista.with_condition("client_id", FilterOp::Eq, "7")?)
        .build()
        .await?;
    until("B holds x", async || holds(&b, "x").await).await;

    feed.push(updated("x", "8", "10"));
    until("A holds x as client 8", async || {
        cached_cell(&a, "x", "client_id").await.as_deref() == Some("8")
    })
    .await;
    until("B drops x", async || !holds(&b, "x").await).await;

    feed.push(VistaChange::Inserted {
        id: "y".into(),
        value: invoice("y", "7", "1"),
    });
    until("B shows y", async || holds(&b, "y").await).await;
    Ok(())
}

/// A narrowed live Dio turns a row leaving its set into a delete of its
/// own. The others get the row as pushed, so one over the whole table
/// keeps it.
#[tokio::test]
async fn a_narrowed_live_follow_publishes_no_synthetic_delete() -> Result<()> {
    let lens = memory_lens(copy_master());
    let (vista, feed) = live_master();
    let c = lens.dio(vista.clone()).build().await?;
    let a = lens
        .dio(vista.with_condition("client_id", FilterOp::Eq, "7")?)
        .live()
        .build()
        .await?;
    until("A holds x", async || holds(&a, "x").await).await;

    feed.push(updated("x", "8", "10"));

    until("A drops x", async || !holds(&a, "x").await).await;
    until("C holds x as client 8", async || {
        cached_cell(&c, "x", "client_id").await.as_deref() == Some("8")
    })
    .await;
    Ok(())
}

#[tokio::test]
async fn one_subscription_serves_every_live_dio_over_the_table() -> Result<()> {
    let lens = memory_lens(copy_master());
    let (vista, feed) = live_master();
    let a = feed_on(&lens, vista).await;
    let seven = narrowed_live(&a, "7").await;
    let eight = narrowed_live(&a, "8").await;
    let nine = narrowed_live(&a, "9").await;
    assert_eq!(feed.subscriptions(), 1);

    feed.push(updated("x", "9", "10"));

    until("x moves from 7 to 9", async || {
        !holds(&seven, "x").await && holds(&nine, "x").await
    })
    .await;
    assert!(!holds(&eight, "x").await);
    assert!(holds(&eight, "z").await);
    Ok(())
}

#[tokio::test]
async fn a_follower_keeps_its_feed_alive() -> Result<()> {
    let lens = memory_lens(copy_master());
    let (vista, feed) = live_master();
    let a = feed_on(&lens, vista).await;
    let b = narrowed_live(&a, "7").await;
    drop(a);

    feed.push(updated("x", "7", "42"));

    until("B repaints x", async || {
        cached_cell(&b, "x", "amount").await.as_deref() == Some("42")
    })
    .await;
    assert_eq!(feed.subscriptions(), 1);
    Ok(())
}

#[tokio::test]
async fn without_a_feed_a_narrowed_live_dio_subscribes_itself() -> Result<()> {
    let lens = memory_lens(copy_master());
    let (vista, feed) = live_master();
    let _b = lens
        .dio(vista.with_condition("client_id", FilterOp::Eq, "7")?)
        .live()
        .build()
        .await?;
    assert_eq!(feed.subscriptions(), 1);
    Ok(())
}

/// The feed's stream ends. It resubscribes and re-reads the set, and what
/// changed during the gap reaches the Dios that follow it.
#[tokio::test]
async fn a_feed_recovery_reaches_followers() -> Result<()> {
    let lens = memory_lens(copy_master());
    let (vista, feed) = live_master();
    let a = feed_on(&lens, vista).await;
    let b = narrowed_live(&a, "7").await;

    feed.end_streams();
    // Changed while nobody is subscribed: only the re-read can find it.
    feed.store.set_record("x", invoice("x", "7", "77"));

    until("B sees the change made during the gap", async || {
        cached_cell(&b, "x", "amount").await.as_deref() == Some("77")
    })
    .await;
    assert_eq!(feed.subscriptions(), 2, "the feed resubscribed");
    Ok(())
}

/// A feed with no eager start never holds a whole copy, so its re-read
/// after the gap has no diff to publish. Its followers are told to re-read
/// their own sets instead.
#[tokio::test]
async fn a_lazy_feed_recovery_makes_followers_reread() -> Result<()> {
    let lens = Lens::builder().cache(Cache::memory()).build()?;
    let (vista, feed) = live_master();
    let a = feed_on(&lens, vista).await;
    let b = narrowed_live(&a, "7").await;
    let mut b_bus = b.subscribe_events();

    feed.end_streams();

    let reread = async { while !matches!(b_bus.recv().await, Ok(DioEvent::Refreshing)) {} };
    tokio::time::timeout(WITHIN, reread)
        .await
        .expect("B never re-read its set");
    assert_eq!(feed.subscriptions(), 2, "the feed resubscribed");
    Ok(())
}
