//! Phase-1 steps: prove the Mock backend + Lens + Dio path end-to-end.

use cucumber::{gherkin::Step, given, then, when};
use vantage_dataset::traits::ReadableValueSet;

use crate::bdd_support::{
    backend::{BackendKind, MasterRows},
    sqlite_runtime::dispatch,
    world::DioramaWorld,
};

#[given(regex = r"^a master with rows$")]
async fn master_with_rows(w: &mut DioramaWorld, step: &Step) {
    let table = step
        .table
        .as_ref()
        .expect("data table required for `a master with rows`");
    let rows = MasterRows::from_table("items", table);
    let master = rows.build_master_for(w).await.expect("build master vista");
    w.master = Some(master);
}

#[given("a lens with on_start that copies master to cache")]
async fn lens_with_on_start_load(w: &mut DioramaWorld) {
    w.lens_builder.on_start_load_master = true;
}

#[when("the dio is created")]
async fn create_dio(w: &mut DioramaWorld) {
    let cache = w.cache_source();
    let lens = w
        .lens_builder
        .build(cache, &w.spies, w.backend)
        .expect("build lens");
    let master = w.master.take().expect("master not set");
    let dio = lens.make_dio(master).await.expect("make_dio");
    w.start_recorder(dio.subscribe_events());
    w.lens = Some(lens);
    w.dio = Some(dio);
}

#[then(regex = r"^the master responds to list with (\d+) rows?$")]
async fn master_list_count(w: &mut DioramaWorld, n: u64) {
    let dio = w.dio.as_ref().expect("dio not created").clone();
    let rows = if w.backend == BackendKind::Sqlite {
        dispatch(async move { dio.master().list_values().await }).await
    } else {
        dio.master().list_values().await
    }
    .expect("list master");
    assert_eq!(
        rows.len() as u64,
        n,
        "expected {n} master rows, got {}",
        rows.len()
    );
}
