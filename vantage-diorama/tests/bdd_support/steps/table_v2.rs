//! Steps for the v2 `TableScenery` features — the total a paged loader
//! states through `set_total`, sparse rows, viewport-driven chunk loading.

mod assertions;
mod driving;

use std::sync::atomic::Ordering;

use cucumber::given;

use crate::bdd_support::backend::MasterRows;
use crate::bdd_support::world::DioramaWorld;
use crate::bdd_support::world::config::{OnLoadChunkKind, OnStartLoad, TotalProviderKind};

#[given(regex = r"^a master with (\d+) rows$")]
async fn master_with_n_rows(w: &mut DioramaWorld, n: usize) {
    let rows = MasterRows::synthetic(n);
    let master = rows.build_master_for(w).await.expect("build master vista");
    w.master = Some(master);
}

#[given(regex = r"^a lens with on_start that copies the first (\d+) rows to cache$")]
async fn on_start_first_n(w: &mut DioramaWorld, n: usize) {
    w.lens_builder.on_start_load_kind = OnStartLoad::FirstN(n);
    // Don't fire the auto-refresh on open — these scenarios pre-seed
    // the cache and assert specific event counts.
    w.lens_builder.refresh_on_open = Some(false);
}

#[given("a paged loader that states the master count through set_total")]
async fn total_provider_master(w: &mut DioramaWorld) {
    w.lens_builder.total_provider_kind = TotalProviderKind::Master;
}

#[given("a paged loader that states the master count through set_total and records each count")]
async fn total_provider_recorded(w: &mut DioramaWorld) {
    w.lens_builder.total_provider_kind = TotalProviderKind::MasterRecorded;
}

#[given("no loader states a total")]
async fn no_total_provider(w: &mut DioramaWorld) {
    w.lens_builder.total_provider_kind = TotalProviderKind::Unset;
}

#[given("an on_load_chunk callback that pulls the requested range from master")]
async fn on_load_chunk_pull(w: &mut DioramaWorld) {
    w.lens_builder.on_load_chunk_kind = OnLoadChunkKind::PullFromMaster;
}

// v4 spec wording for the same callback. Kept as a separate phrase so
// the v2 features (which use the original wording) keep working unchanged.
#[given("a lens with on_load_chunk that pulls the requested range from master")]
async fn lens_with_on_load_chunk_pull(w: &mut DioramaWorld) {
    w.lens_builder.on_load_chunk_kind = OnLoadChunkKind::PullFromMaster;
}

#[given("on_load_chunk errors on the next call")]
async fn on_load_chunk_error_once(w: &mut DioramaWorld) {
    w.spies
        .on_load_chunk_error_once
        .store(true, Ordering::SeqCst);
}

#[given("an on_load_chunk callback that records calls")]
async fn on_load_chunk_record(w: &mut DioramaWorld) {
    w.lens_builder.on_load_chunk_kind = OnLoadChunkKind::RecordCalls;
}

#[given("an on_load_chunk callback that always errors")]
async fn on_load_chunk_error(w: &mut DioramaWorld) {
    w.lens_builder.on_load_chunk_kind = OnLoadChunkKind::AlwaysError;
}
