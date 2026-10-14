//! Drain, not drop: queued flashes land after every handle and caller is
//! gone. Needs `test-util` for the write worker's join handle.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_diorama::{Cache, ChangeFlash, FlashOutcome, Lens};
use vantage_vista::mocks::MockShell;

use super::vista_over;
use crate::support::flash::staged;
use crate::support::record;

/// Queued flashes land after every handle drops, and after their callers
/// stop waiting: the worker then exits on its own.
#[tokio::test]
async fn queued_flashes_survive_dropping_every_handle() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let shell = MockShell::new();
    let log = Arc::new(vista_over(&shell, "tasks"));
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer({
            let (gate, log) = (gate.clone(), log.clone());
            move |_cx, flash| {
                let (gate, log) = (gate.clone(), log.clone());
                async move {
                    gate.acquire().await.expect("gate open").forget();
                    flash.active_record(log.as_ref())?.save().await?;
                    Ok(FlashOutcome::applied())
                }
            }
        })
        .build()?;
    let dio = lens
        .dio(vista_over(&MockShell::new(), "tasks"))
        .build()
        .await?;
    let worker = dio.take_write_worker_handle().await.expect("worker handle");
    let mut bus = dio.subscribe_events();

    let callers: Vec<_> = ["t1", "t2"]
        .into_iter()
        .map(|id| {
            let dio = dio.clone();
            tokio::spawn(async move {
                dio.flash(ChangeFlash::insert(id, record(&[("id", id)])))
                    .await
            })
        })
        .collect();
    staged(&mut bus, &["t1", "t2"]).await;
    for caller in &callers {
        caller.abort();
    }
    drop((dio, bus, lens));
    gate.add_permits(2);

    // The queued flashes keep the pipeline alive until they land; the
    // worker then exits cleanly on its own.
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("worker exited after draining")
        .expect("worker task completed");
    assert_eq!(shell.len(), 2, "both queued writes landed");
    Ok(())
}
