//! Phase-5a steps: `refresh_every` skip-first under virtual time, plus
//! manual `dio.refresh()`. Time advance only works because `bdd.rs`
//! starts the runtime with `start_paused = true`.

use std::sync::atomic::Ordering;
use std::time::Duration;

use cucumber::{given, then, when};

use crate::bdd_support::world::DioramaWorld;

#[given(regex = r"^a refresh interval of (\d+) seconds$")]
async fn refresh_interval(w: &mut DioramaWorld, secs: u64) {
    w.lens_builder.refresh_every = Some(Duration::from_secs(secs));
}

#[given("an on_refresh callback that records calls")]
async fn register_on_refresh(w: &mut DioramaWorld) {
    w.lens_builder.register_on_refresh = true;
}

#[when(regex = r"^(\d+) seconds? pass(?:es)?$")]
async fn time_passes(w: &mut DioramaWorld, secs: u64) {
    tokio::time::advance(Duration::from_secs(secs)).await;
    w.settle().await;
}

#[then(regex = r"^on_refresh has been called (\d+) times?$")]
async fn assert_refresh_count(w: &mut DioramaWorld, expected: u64) {
    let got = w.spies.on_refresh.load(Ordering::SeqCst);
    assert_eq!(got, expected, "expected on_refresh={expected}, got {got}");
}

#[when("dio.refresh is called")]
async fn manual_refresh(w: &mut DioramaWorld) {
    let dio = w.dio.as_ref().expect("dio not created");
    dio.refresh().await.expect("dio.refresh");
    w.settle().await;
}
