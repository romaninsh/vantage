//! Waiting for a condition, or for a scenery's first rows, and reading a
//! row's status.

use std::sync::Arc;
use std::time::Duration;

use vantage_diorama::{Dio, RowStatus, TableScenery};

/// Poll `holds` every 5ms until it is true or `within` elapses. Returns
/// whether it came true. Bounds a wait; it is not a speed assertion.
pub async fn eventually(within: Duration, holds: impl Fn() -> bool) -> bool {
    tokio::time::timeout(within, async {
        while !holds() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok()
}

/// Poll until `f` holds, or panic after ~1s naming `label`. Real clock; small
/// sleeps yield to the spawned viewport / list tasks.
pub async fn poll_until(label: &str, f: impl Fn() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition '{label}' not met within timeout");
}

/// The status of the row at `idx`, if the scenery has one there.
pub fn status_of(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<RowStatus> {
    scenery.row(idx).map(|r| r.status.clone())
}

/// Open a two-pass scenery and wait until its first rows have landed.
///
/// `open()` no longer implies data: a two-pass view runs its list page detached
/// so a slow source cannot freeze the caller (a UI thread, in the app). Waiting
/// on [`LoadState`] is the supported way to say "once it has something" —
/// a fixed sleep would be a speed assertion, and asserting straight after
/// `open()` is a race the test would sometimes win.
///
/// [`LoadState`]: vantage_diorama::LoadState
pub async fn open_listed(dio: &Dio, page_size: usize) -> Arc<dyn TableScenery> {
    let scenery = dio
        .table_scenery()
        .page_size(page_size)
        .open()
        .await
        .expect("scenery opens");
    wait_listed(&scenery).await;
    scenery
}

/// Wait for a scenery to leave [`LoadState::Loading`].
///
/// Driven by the generation watch rather than a poll: every path that settles a
/// scenery bumps the generation immediately afterwards, so the signal already
/// exists and sleeping between checks would only add latency to it. Subscribe
/// before the first check, or a load that finishes in between is a bump nobody
/// is listening for.
///
/// The timeout bounds a hang; it is not an assertion about speed.
///
/// [`LoadState::Loading`]: vantage_diorama::LoadState::Loading
pub async fn wait_listed(scenery: &Arc<dyn TableScenery>) {
    let mut generations = scenery.subscribe();
    let settled = async {
        while scenery.load_state() == vantage_diorama::LoadState::Loading {
            generations
                .changed()
                .await
                .expect("generation watch closed");
        }
    };
    tokio::time::timeout(Duration::from_secs(3), settled)
        .await
        .expect("scenery never left LoadState::Loading");
}
