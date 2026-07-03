//! Cucumber entry point. Each scenario gets a fresh [`LiveFolderWorld`] on a
//! single-threaded tokio runtime. The sim ticks real `SystemTime::now()` so
//! scenarios use small backfills (1–2 hours) to keep wall time bounded.

use cucumber::World;

mod world;

use world::LiveFolderWorld;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    LiveFolderWorld::cucumber()
        .max_concurrent_scenarios(1)
        .fail_on_skipped()
        .run_and_exit("tests/features")
        .await;
}
