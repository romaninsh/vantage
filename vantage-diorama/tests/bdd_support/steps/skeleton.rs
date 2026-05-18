//! Phase-1 steps: prove the Mock backend + Lens + Dio path end-to-end.

use cucumber::{gherkin::Step, given, then, when};
use vantage_dataset::traits::ReadableValueSet;

use crate::bdd_support::{
    backend::{BackendKind, MasterRows},
    world::DioramaWorld,
};

#[given(regex = r"^a master with rows$")]
async fn master_with_rows(w: &mut DioramaWorld, step: &Step) {
    let table = step
        .table
        .as_ref()
        .expect("data table required for `a master with rows`");
    let rows = MasterRows::from_table("items", table);
    let backend = w.backend;
    let master = rows
        .build_master(backend)
        .await
        .expect("build master vista");
    w.master = Some(master);
}

#[given("a lens with on_start that copies master to cache")]
async fn lens_with_on_start_load(w: &mut DioramaWorld) {
    w.lens_builder.on_start_load_master = true;
}

#[given(regex = r"^the backend is (mock|csv|sqlite)$")]
async fn select_backend(w: &mut DioramaWorld, kind: String) {
    w.backend = BackendKind::parse(&kind);
}

#[when("the dio is created")]
async fn create_dio(w: &mut DioramaWorld) {
    let cache_path = w.tmp_path().join("cache.redb");
    let lens = w
        .lens_builder
        .build(cache_path, &w.spies)
        .expect("build lens");
    let master = w.master.take().expect("master not set");
    let dio = lens.make_dio(master).await.expect("make_dio");
    w.start_recorder(dio.subscribe_events());
    w.lens = Some(lens);
    w.dio = Some(dio);
}

#[then(regex = r"^the cache contains (\d+) rows?$")]
async fn cache_contains_n(w: &mut DioramaWorld, n: u64) {
    let dio = w.dio.as_ref().expect("dio not created");
    let count = dio.cache().count().await.expect("cache count") as u64;
    assert_eq!(count, n, "expected {n} cached rows, got {count}");
}

#[then(regex = r"^the master responds to list with (\d+) rows?$")]
async fn master_list_count(w: &mut DioramaWorld, n: u64) {
    let dio = w.dio.as_ref().expect("dio not created");
    let rows = dio.master().list_values().await.expect("list master");
    assert_eq!(
        rows.len() as u64,
        n,
        "expected {n} master rows, got {}",
        rows.len()
    );
}
