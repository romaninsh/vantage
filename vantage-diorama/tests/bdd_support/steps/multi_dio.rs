//! Phase-5b steps: a single Lens hosting multiple Dios, each bound to a
//! differently-named master and claiming a distinct cache table.

use cucumber::{gherkin::Step, given, then, when};

use crate::bdd_support::{backend::MasterRows, world::DioramaWorld};

#[given(regex = r#"^a master named "([^"]+)" with rows$"#)]
async fn named_master(w: &mut DioramaWorld, name: String, step: &Step) {
    let table = step
        .table
        .as_ref()
        .expect("data table required for `a master named …`");
    let rows = MasterRows::from_table(&name, table);
    let master = rows
        .build_master_for(w)
        .await
        .expect("build named master");
    w.named_masters.insert(name, master);
}

#[when(regex = r#"^the dio for "([^"]+)" is created$"#)]
async fn create_named_dio(w: &mut DioramaWorld, name: String) {
    let cache_path = w.tmp_path().join("cache.redb");
    // Reuse the Lens across all named dios under this scenario.
    if w.lens.is_none() {
        let lens = w
            .lens_builder
            .build(cache_path, &w.spies)
            .expect("build lens");
        w.lens = Some(lens);
    }
    let lens = w.lens.as_ref().expect("lens not built").clone();
    let master = w
        .named_masters
        .remove(&name)
        .unwrap_or_else(|| panic!("no master named {name}"));
    let dio = lens.make_dio(master).await.expect("make_dio");
    w.start_recorder(dio.subscribe_events());
    w.named_dios.insert(name, dio);
}

#[then(regex = r#"^the "([^"]+)" cache contains (\d+) rows?$"#)]
async fn named_cache_contains(w: &mut DioramaWorld, name: String, expected: u64) {
    let dio = w
        .named_dios
        .get(&name)
        .unwrap_or_else(|| panic!("no dio named {name}"));
    let got = dio.cache().count().await.expect("cache count") as u64;
    assert_eq!(
        got, expected,
        "expected {expected} rows in {name} cache, got {got}"
    );
}

#[then("the two cache tables are distinct")]
async fn cache_tables_distinct(w: &mut DioramaWorld) {
    let names: Vec<&str> = w
        .named_dios
        .values()
        .map(|d| d.cache_table_name())
        .collect();
    assert!(names.len() >= 2, "need at least two dios to compare");
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        names.len(),
        "cache table names overlap: {names:?}"
    );
}
