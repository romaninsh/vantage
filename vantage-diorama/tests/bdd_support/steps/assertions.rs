//! Shared assertion steps — kept out of phase-specific files so future
//! phases (event_path, refresh) can reuse the snapshot infrastructure
//! without copy-paste.

use cucumber::{then, when};

use crate::bdd_support::world::DioramaWorld;

#[when(regex = r"^I wait for (\d+) events?$")]
async fn wait_for_events(w: &mut DioramaWorld, expected: usize) {
    // Yield + tiny virtual-time advance until the event log reaches the
    // expected size. The recorder task lives on the same paused
    // single-threaded runtime, so it makes progress only when this task
    // yields. Bounded so a missed event becomes a test failure, not a
    // hang.
    const MAX_POLLS: usize = 2_000;
    const YIELDS_PER_POLL: usize = 20;
    for _ in 0..MAX_POLLS {
        if w.snapshot_events().await.len() >= expected {
            return;
        }
        // Multiple yields per advance — the viewport pipeline has a
        // long chain of awaits (debounce timer → callback → 100×
        // sink.push → cache spawn_blocking → bump generation →
        // event emit). One yield per advance only gives one task one
        // cycle, so chains starve. Batching yields lets each task
        // chip away before time moves on.
        for _ in 0..YIELDS_PER_POLL {
            tokio::task::yield_now().await;
        }
        tokio::time::advance(std::time::Duration::from_millis(1)).await;
    }
    let got = w.snapshot_events().await.len();
    panic!("expected at least {expected} events, got {got} after {MAX_POLLS} polls");
}

#[then(regex = r#"^the event log matches snapshot "([^"]+)"$"#)]
async fn event_log_snapshot(w: &mut DioramaWorld, name: String) {
    // Scenarios that care about a specific event count should pin it
    // explicitly via `When I wait for N events` before snapshotting —
    // otherwise we just capture whatever the recorder has drained so far.
    //
    // Materialise as Debug strings so we don't have to teach DioEvent
    // how to Serialize. The filter strips file:line:col tails that creep
    // in through `vantage_core::error!` so the snapshot is stable across
    // unrelated edits.
    let events: Vec<String> = w
        .snapshot_events()
        .await
        .iter()
        .map(|e| format!("{e:?}"))
        .collect();

    insta::with_settings!({
        filters => vec![
            (r#"[A-Za-z0-9_./\\-]+\.rs:\d+:\d+"#, "[LOC]"),
        ],
        snapshot_path => "../../snapshots",
        snapshot_suffix => name.as_str(),
        prepend_module_to_snapshot => false,
    }, {
        insta::assert_yaml_snapshot!(events);
    });
}
