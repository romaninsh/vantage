//! Cucumber entry point. Each scenario gets a fresh [`DioramaWorld`] on a
//! single-threaded paused-clock tokio runtime — `tokio::time::advance` is
//! the only way time moves, which keeps refresh/timeout scenarios
//! deterministic.

use cucumber::World;

mod bdd_support;

use bdd_support::world::DioramaWorld;

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    DioramaWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit("tests/features")
        .await;
}
