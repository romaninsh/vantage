//! Cucumber entry point. Each scenario gets a fresh [`DioramaWorld`] on a
//! single-threaded paused-clock tokio runtime — `tokio::time::advance` is
//! the only way time moves, which keeps refresh/timeout scenarios
//! deterministic.
//!
//! Scenarios tagged `@wip` (at feature or scenario level) are filtered
//! out — they're future-implementation references whose steps may not
//! exist yet. Drop the tag once a phase lands its steps.

use cucumber::World;

mod bdd_support;

use bdd_support::world::DioramaWorld;

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    DioramaWorld::cucumber()
        .max_concurrent_scenarios(1)
        .fail_on_skipped()
        .filter_run_and_exit("tests/features", |feat, _rule, sc| {
            !feat.tags.iter().chain(sc.tags.iter()).any(|t| t == "wip")
        })
        .await;
}
