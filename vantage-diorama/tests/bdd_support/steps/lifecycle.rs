//! Phase-2 steps: Lens lifecycle — `on_start_blocking` true/false and
//! the last-Dio-drop → worker-exit contract.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};

use crate::bdd_support::world::DioramaWorld;

#[given("a gated on_start that copies master to cache")]
async fn gated_on_start(w: &mut DioramaWorld) {
    w.lens_builder.on_start_load_master = true;
    let _ = w.install_on_start_gate();
}

#[given(regex = r"^on_start_blocking is (true|false)$")]
async fn set_on_start_blocking(w: &mut DioramaWorld, val: String) {
    w.lens_builder.on_start_blocking = Some(val == "true");
}

#[when("I spawn the dio build")]
async fn spawn_dio_build(w: &mut DioramaWorld) {
    let lens = w.build_lens();
    let master = w.master.take().expect("master not set");
    w.lens = Some(lens.clone());
    let handle = tokio::spawn(async move { lens.dio(master).build().await });
    w.pending_dio = Some(handle);
    w.settle().await;
}

#[then("the dio build is still pending")]
async fn dio_build_pending(w: &mut DioramaWorld) {
    let handle = w.pending_dio.as_ref().expect("no pending dio build");
    assert!(
        !handle.is_finished(),
        "expected the dio build to be parked on the on_start gate, but it finished early"
    );
}

#[then("the dio build completes")]
async fn dio_build_completes(w: &mut DioramaWorld) {
    let handle = w.pending_dio.take().expect("no pending dio build");
    let dio = handle
        .await
        .expect("dio build task panicked")
        .expect("dio build returned err");
    w.start_recorder(dio.subscribe_events());
    w.dio = Some(dio);
}

#[when("I release on_start")]
async fn release_on_start(w: &mut DioramaWorld) {
    w.release_on_start_gate();
    w.settle().await;
}

#[then(regex = r"^on_start has been called (\d+) times?$")]
async fn assert_on_start_count(w: &mut DioramaWorld, n: u64) {
    let got = w.spies.on_start.load(Ordering::SeqCst);
    assert_eq!(got, n, "expected on_start={n}, got {got}");
}

#[when("I capture the write worker handle")]
async fn capture_worker_handle(w: &mut DioramaWorld) {
    let dio = w.dio.as_ref().expect("dio not created");
    let handle = dio
        .take_write_worker_handle()
        .await
        .expect("write worker handle already taken");
    w.worker_handle = Some(handle);
}

#[when("I drop the dio")]
async fn drop_dio(w: &mut DioramaWorld) {
    // Drop the event recorder before the dio so the broadcast channel
    // doesn't keep the inner state alive via the recorder task.
    if let Some(rec) = w.recorder.take() {
        rec.abort();
        let _ = rec.await;
    }
    let _ = w.dio.take();
    w.extra_dios.clear();
    // Drop the Arc<Lens> too — the Dio holds a clone inside DioInner,
    // but the outer Arc the test holds also keeps the cache backend alive.
    let _ = w.lens.take();
    w.settle().await;
}

#[then("the write worker exits cleanly")]
async fn worker_exits(w: &mut DioramaWorld) {
    let handle = w.worker_handle.take().expect("no captured worker handle");
    handle.await.expect("write worker task panicked");
}
