//! Phase-4 steps: upstream `ChangeEvent` → `on_event` → cache, internal
//! bus fanout, and the `TableScenery` generation-bump contract.

use ciborium::Value as CborValue;
use cucumber::{given, then, when};
use vantage_diorama::ChangeEvent;
use vantage_types::Record;

use crate::bdd_support::world::{DioramaWorld, OnEventMode};

#[given("an on_event callback that calls dio.patched")]
async fn on_event_patched(w: &mut DioramaWorld) {
    w.lens_builder.on_event_mode = OnEventMode::PatchedFromUpdate;
}

#[when(regex = r#"^a ChangeEvent::Updated arrives for id "([^"]+)" with title "([^"]+)"$"#)]
async fn change_event_updated(w: &mut DioramaWorld, id: String, title: String) {
    let dio = w.dio.as_ref().expect("dio not created");
    let mut rec: Record<CborValue> = Record::new();
    rec.insert("title".to_string(), CborValue::Text(title));
    dio.handle_event(ChangeEvent::Updated { id, new: Some(rec) })
        .await
        .expect("handle_event");
    w.settle().await;
}

#[when(regex = r#"^dio\.invalidate_record is called for "([^"]+)"$"#)]
async fn invalidate_record(w: &mut DioramaWorld, id: String) {
    let dio = w.dio.as_ref().expect("dio not created");
    dio.invalidate_record(id);
    w.settle().await;
}

#[when("dio.invalidate_all is called")]
async fn invalidate_all(w: &mut DioramaWorld) {
    let dio = w.dio.as_ref().expect("dio not created");
    dio.invalidate_all();
    w.settle().await;
}

#[then(regex = r#"^the cache record "([^"]+)" has title "([^"]+)"$"#)]
async fn cache_record_title(w: &mut DioramaWorld, id: String, expected: String) {
    let dio = w.dio.as_ref().expect("dio not created");
    let row = dio
        .cache()
        .get_value(&id)
        .await
        .expect("cache get_value")
        .unwrap_or_else(|| panic!("cache has no record {id}"));
    let got = row
        .get("title")
        .and_then(|v| match v {
            CborValue::Text(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert_eq!(
        got, expected,
        "cache record {id}.title: want {expected}, got {got}"
    );
}

#[when("the table scenery is opened")]
async fn open_table_scenery(w: &mut DioramaWorld) {
    let dio = w.dio.as_ref().expect("dio not created");
    let scenery = dio
        .table_scenery()
        .open()
        .await
        .expect("open table scenery");
    w.scenery = Some(scenery);
    w.settle().await;
}

#[then(regex = r"^the table scenery generation is (\d+)$")]
async fn scenery_generation_is(w: &mut DioramaWorld, expected: u64) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    let mut rx = scenery.subscribe();
    // Poll the watch channel for the expected value. The reload pipeline
    // crosses `spawn_blocking` (redb), so a few `yield_now()`s aren't
    // enough; bounded busy-poll with tiny advances drives the runtime.
    for _ in 0..200 {
        let got: u64 = (*rx.borrow_and_update()).into();
        if got == expected {
            return;
        }
        if got > expected {
            panic!("TableScenery generation overshoot: want {expected}, got {got}");
        }
        tokio::time::advance(std::time::Duration::from_micros(1)).await;
    }
    let got: u64 = (*rx.borrow()).into();
    panic!("TableScenery generation: want {expected}, got {got} after 200 advances");
}
