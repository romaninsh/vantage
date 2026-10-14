//! Stacked flashes on one row: a rollback restores the last confirmed row,
//! never an earlier flash's unconfirmed value, and a commit never overwrites
//! a later flash still staged on the row.

use std::sync::Arc;

use ciborium::Value as CborValue;
use tokio::sync::Semaphore;
use vantage_core::{Result, error};
use vantage_diorama::{Cache, ChangeFlash, Dio, FlashOutcome, Lens};
use vantage_vista::{Column, FilterOp, Vista, VistaMetadata, mocks::MockShell};

mod support;

use support::flash::{cached, seeded_dio, staged};
use support::{record, text};

type Pending = tokio::task::JoinHandle<Result<FlashOutcome>>;

/// A Lens whose writer waits for a permit from `gate`, then refuses a flash
/// that sets `n` to one of `refused`.
fn refusing_lens(gate: Arc<Semaphore>, refused: &'static [&'static str]) -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::memory())
        .writer(move |_cx, flash| {
            let gate = gate.clone();
            let refuse = refused
                .iter()
                .any(|r| flash.patch().get("n") == Some(&text(r)));
            async move {
                gate.acquire().await.expect("gate open").forget();
                if refuse {
                    Err(error!("refused"))
                } else {
                    Ok(FlashOutcome::applied())
                }
            }
        })
        .build()
        .expect("lens")
}

/// A master holding `a = {team: red, n: 1}`.
fn counters() -> MockShell {
    MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("id", "String").with_flag("id"))
                .with_column(Column::new("team", "String"))
                .with_column(Column::new("n", "String"))
                .with_id_column("id"),
        )
        .with_record("a", record(&[("id", "a"), ("team", "red"), ("n", "1")]))
}

/// `n: 2` then `n: 3` flashed on `a`, both staged: the first parked in the
/// writer, the second queued behind it.
async fn two_flashes_on_a(dio: &Dio) -> [Pending; 2] {
    let mut bus = dio.subscribe_events();
    let flash = |n: &str| {
        let (dio, patch) = (dio.clone(), record(&[("n", n)]));
        tokio::spawn(async move { dio.flash(ChangeFlash::partial("a", patch)).await })
    };
    let first = flash("2");
    staged(&mut bus, &["a"]).await;
    let second = flash("3");
    staged(&mut bus, &["a"]).await;
    [first, second]
}

async fn n_of(dio: &Dio) -> Result<Option<CborValue>> {
    Ok(cached(dio, "a")
        .await?
        .and_then(|row| row.get("n").cloned()))
}

#[tokio::test]
async fn two_failed_flashes_restore_the_last_confirmed_row() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let lens = refusing_lens(gate.clone(), &["2", "3"]);
    let dio = seeded_dio(&lens, Vista::new("counters", counters())).await?;
    let [first, second] = two_flashes_on_a(&dio).await;

    gate.add_permits(2);
    assert!(first.await.unwrap().is_err());
    assert!(second.await.unwrap().is_err());

    assert_eq!(
        n_of(&dio).await?,
        Some(text("1")),
        "not the first flash's value"
    );
    let scenery = dio.record_scenery("a").open().await?;
    let shown = scenery.record().expect("record present");
    assert_eq!(shown.record.get("n"), Some(&text("1")));
    Ok(())
}

#[tokio::test]
async fn an_earlier_commit_does_not_overwrite_a_later_staged_value() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let lens = refusing_lens(gate.clone(), &["3"]);
    let dio = seeded_dio(&lens, Vista::new("counters", counters())).await?;
    let [first, second] = two_flashes_on_a(&dio).await;

    gate.add_permits(1);
    first.await.unwrap()?;
    assert_eq!(
        n_of(&dio).await?,
        Some(text("3")),
        "the later flash still shows"
    );

    gate.add_permits(1);
    assert!(second.await.unwrap().is_err());
    assert_eq!(n_of(&dio).await?, Some(text("2")), "the confirmed value");
    Ok(())
}

/// On a narrowed Dio a patch of a row the cache doesn't hold stages over the
/// master's whole row. Its commit confirms that whole row, so the later
/// flash's rollback keeps every column.
#[tokio::test]
async fn a_rollback_over_an_uncached_patched_row_keeps_every_column() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let lens = refusing_lens(gate.clone(), &["3"]);
    let red = Vista::new("counters", counters()).with_condition("team", FilterOp::Eq, "red")?;
    let dio = lens.dio(red).build().await?;
    assert!(cached(&dio, "a").await?.is_none(), "the row isn't cached");
    let [first, second] = two_flashes_on_a(&dio).await;

    gate.add_permits(2);
    first.await.unwrap()?;
    assert!(second.await.unwrap().is_err());
    let row = cached(&dio, "a").await?.expect("the row stays");
    assert_eq!(row, record(&[("id", "a"), ("team", "red"), ("n", "2")]));
    Ok(())
}
