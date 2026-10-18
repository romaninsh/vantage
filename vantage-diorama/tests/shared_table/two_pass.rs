//! Use cases 1 to 5 over two-pass Dios: A lists the master and merges each
//! row's `note` from a detail source, and B shares A's augmentations.

use std::sync::Arc;

use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{ChangeFlash, Dio, RecordStatus, TableScenery};
use vantage_vista::{Condition, FilterOp};

use crate::fixtures::{Pair, cell, holds, ids, invoice, patch, until};
use crate::support::{open_listed, record};

/// The two-pass pair, A's rows listed into its cache.
async fn listed_pair() -> Pair {
    let pair = Pair::two_pass().await;
    open_listed(&pair.a, 10).await;
    pair
}

/// `dio`'s two-pass view, listed, its first rows on screen so the detail
/// pass hydrates them.
async fn shown(dio: &Dio) -> Arc<dyn TableScenery> {
    let view = open_listed(dio, 10).await;
    view.set_viewport(0..10, 0..10);
    view
}

#[tokio::test]
async fn two_pass_case1_moved_out() -> Result<()> {
    let Pair { a, b, .. } = listed_pair().await;
    let b_view = shown(&b).await;
    until("B lists x", async || ids(&b_view) == ["x"]).await;

    a.flash(patch("x", &[("client_id", "8")])).await?;

    until("B's view drops x", async || ids(&b_view).is_empty()).await;
    assert!(!holds(&b, "x").await);
    Ok(())
}

#[tokio::test]
async fn two_pass_case2_read_by_id() -> Result<()> {
    let Pair { a, b, .. } = listed_pair().await;
    let b_view = shown(&b).await;
    until("B lists x", async || ids(&b_view) == ["x"]).await;

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
async fn two_pass_case3_repaint() -> Result<()> {
    let Pair { a, b, .. } = listed_pair().await;
    let b_view = shown(&b).await;
    until("B shows x's note", async || {
        cell(&b_view, "x", "note").as_deref() == Some("n-x")
    })
    .await;

    a.flash(patch("x", &[("amount", "11")])).await?;

    until("B repaints x, keeping its note", async || {
        cell(&b_view, "x", "amount").as_deref() == Some("11")
            && cell(&b_view, "x", "note").as_deref() == Some("n-x")
    })
    .await;
    Ok(())
}

#[tokio::test]
async fn two_pass_case4_insert() -> Result<()> {
    let Pair { a, b, .. } = listed_pair().await;
    let b_view = shown(&b).await;
    until("B lists x", async || ids(&b_view) == ["x"]).await;

    a.flash(ChangeFlash::insert(
        "y",
        record(&[("id", "y"), ("client_id", "7"), ("amount", "3")]),
    ))
    .await?;

    until("B lists y with its note", async || {
        cell(&b_view, "y", "note").as_deref() == Some("n-y")
    })
    .await;
    Ok(())
}

#[tokio::test]
async fn two_pass_case5_delete() -> Result<()> {
    let Pair { a, b, .. } = listed_pair().await;
    let b_view = shown(&b).await;
    until("B lists x", async || ids(&b_view) == ["x"]).await;

    a.flash(ChangeFlash::delete("x")).await?;

    until("B's view drops x", async || ids(&b_view).is_empty()).await;
    assert!(!holds(&b, "x").await);
    Ok(())
}

/// The master changes `x` behind both Dios' backs. A's refresh pass finds
/// it, and B, which did nothing, repaints `x` and keeps its note.
#[tokio::test]
async fn two_pass_case7_a_refresh_on_a_reaches_b() -> Result<()> {
    let Pair { a, b, shell } = listed_pair().await;
    let b_view = shown(&b).await;
    until("B shows x's note", async || {
        cell(&b_view, "x", "note").as_deref() == Some("n-x")
    })
    .await;

    shell.set_record("x", invoice("x", "7", "12"));
    a.refresh().await?;

    until("B repaints x, keeping its note", async || {
        cell(&b_view, "x", "amount").as_deref() == Some("12")
            && cell(&b_view, "x", "note").as_deref() == Some("n-x")
    })
    .await;
    Ok(())
}

/// A row A writes before its detail pass reached it lacks `note`. A Dio
/// whose set is judged by `note` must not decide on that gap: it re-reads
/// the row through its master once, and keeps it.
#[tokio::test]
async fn an_incomplete_row_is_rechecked_not_judged() -> Result<()> {
    // A lists nothing, so the row it writes is the patch over no detail.
    let Pair { a, b, shell } = Pair::two_pass().await;
    drop(b);
    let noted = a
        .derive()
        .with_condition(Condition::new("note", FilterOp::Eq, "n-x"))
        .build()
        .await?;
    let view = shown(&noted).await;
    until("the noted Dio lists x", async || ids(&view) == ["x"]).await;
    let gets = shell.calls().get;

    a.flash(patch("x", &[("amount", "11")])).await?;

    until("the noted Dio repaints x", async || {
        cell(&view, "x", "amount").as_deref() == Some("11")
    })
    .await;
    assert_eq!(shell.calls().get - gets, 1, "one re-read of x");
    assert_eq!(ids(&view), ["x"]);
    Ok(())
}
