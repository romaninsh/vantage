//! Phase-3 steps: write-queue routing through `on_write` (vs the default
//! to-master path), capability lifting on the facade, and the
//! `WriteFailed` event published when a callback errors.

use ciborium::Value as CborValue;
use cucumber::{gherkin::Step, given, then, when};
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_types::Record;

use crate::bdd_support::{
    backend::BackendKind, sqlite_runtime::dispatch, world::DioramaWorld, world::OnWriteMode,
};

/// Parse a gherkin data table whose first row is the header (with an `id`
/// column) and remaining rows are records. Returns `(id, record)` pairs
/// with the id column stripped from the record body — matching the shape
/// `WritableValueSet::insert_value(&id, &rec)` expects.
fn parse_record_table(step: &Step, step_phrase: &str) -> Vec<(String, Record<CborValue>)> {
    let table = step
        .table
        .as_ref()
        .unwrap_or_else(|| panic!("data table required for `{step_phrase}`"));
    let header = table.rows.first().expect("header row").clone();
    let id_idx = header
        .iter()
        .position(|c| c == "id")
        .expect("data table missing required `id` header");
    let mut out = Vec::with_capacity(table.rows.len().saturating_sub(1));
    for row in table.rows.iter().skip(1) {
        let id = row[id_idx].clone();
        let mut rec: Record<CborValue> = Record::new();
        for (i, val) in row.iter().enumerate() {
            if i == id_idx {
                continue;
            }
            rec.insert(header[i].clone(), CborValue::Text(val.clone()));
        }
        out.push((id, rec));
    }
    out
}

#[given("an on_write callback that records calls")]
async fn on_write_records(w: &mut DioramaWorld) {
    w.lens_builder.on_write_mode = OnWriteMode::Pass;
}

#[given("an on_write callback that always errors")]
async fn on_write_errors(w: &mut DioramaWorld) {
    w.lens_builder.on_write_mode = OnWriteMode::Error;
}

#[given("an on_write callback that mirrors to master and cache")]
async fn on_write_mirrors(w: &mut DioramaWorld) {
    w.lens_builder.on_write_mode = OnWriteMode::Mirror;
}

#[when("the write queue drains")]
async fn drain_write_queue(_w: &mut DioramaWorld) {
    // The mirror path crosses redb's `spawn_blocking` on both master and
    // cache writes. Yielding alone leaves the blocking-pool waker
    // unfulfilled; tiny virtual-time advances tick all wakers, including
    // the ones from the blocking pool.
    for _ in 0..200 {
        tokio::time::advance(std::time::Duration::from_micros(1)).await;
    }
}

#[when("I insert via the facade")]
async fn insert_via_facade(w: &mut DioramaWorld, step: &Step) {
    let dio = w.dio.as_ref().expect("dio not created");
    let facade = dio.vista();
    for (id, rec) in parse_record_table(step, "I insert via the facade") {
        facade
            .insert_value(&id, &rec)
            .await
            .expect("facade insert enqueue");
    }
    // Give the write worker a chance to drain the queue and (on error)
    // publish WriteFailed before the next step asserts on the event log.
    w.settle().await;
}

#[when("I replace via the facade")]
async fn replace_via_facade(w: &mut DioramaWorld, step: &Step) {
    let dio = w.dio.as_ref().expect("dio not created");
    let facade = dio.vista();
    for (id, rec) in parse_record_table(step, "I replace via the facade") {
        facade
            .replace_value(&id, &rec)
            .await
            .expect("facade replace enqueue");
    }
    w.settle().await;
}

#[when("I patch via the facade")]
async fn patch_via_facade(w: &mut DioramaWorld, step: &Step) {
    let dio = w.dio.as_ref().expect("dio not created");
    let facade = dio.vista();
    for (id, partial) in parse_record_table(step, "I patch via the facade") {
        facade
            .patch_value(&id, &partial)
            .await
            .expect("facade patch enqueue");
    }
    w.settle().await;
}

#[when(regex = r#"^I delete id "([^"]+)" via the facade$"#)]
async fn delete_via_facade(w: &mut DioramaWorld, id: String) {
    let dio = w.dio.as_ref().expect("dio not created");
    let facade = dio.vista();
    facade.delete(&id).await.expect("facade delete enqueue");
    w.settle().await;
}

#[when("I delete all via the facade")]
async fn delete_all_via_facade(w: &mut DioramaWorld) {
    let dio = w.dio.as_ref().expect("dio not created");
    let facade = dio.vista();
    facade
        .delete_all()
        .await
        .expect("facade delete_all enqueue");
    w.settle().await;
}

#[then(
    regex = r"^the facade capability (can_insert|can_update|can_delete|can_subscribe|can_invalidate|can_count) is (true|false)$"
)]
async fn facade_capability(w: &mut DioramaWorld, flag: String, expected: String) {
    let dio = w.dio.as_ref().expect("dio not created");
    let facade = dio.vista();
    let caps = facade.capabilities();
    let actual = match flag.as_str() {
        "can_insert" => caps.can_insert,
        "can_update" => caps.can_update,
        "can_delete" => caps.can_delete,
        "can_subscribe" => caps.can_subscribe,
        "can_invalidate" => caps.can_invalidate,
        "can_count" => caps.can_count,
        other => panic!("unknown capability flag: {other}"),
    };
    let want = expected == "true";
    assert_eq!(
        actual, want,
        "facade capability {flag}: want {want}, got {actual}"
    );
}

#[then(regex = r"^on_write has been called (\d+) times?$")]
async fn assert_on_write_count(w: &mut DioramaWorld, n: u64) {
    let got = w.spies.on_write.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(got, n, "expected on_write={n}, got {got}");
}

#[then(regex = r"^the master has (\d+) rows?$")]
async fn master_row_count(w: &mut DioramaWorld, n: u64) {
    let dio = w.dio.as_ref().expect("dio not created").clone();
    let rows = if w.backend == BackendKind::Sqlite {
        dispatch(async move { dio.master().list_values().await }).await
    } else {
        dio.master().list_values().await
    }
    .expect("master list");
    assert_eq!(
        rows.len() as u64,
        n,
        "expected {n} master rows, got {}",
        rows.len()
    );
}

#[then(regex = r"^the cache (?:still )?(?:has|contains) (\d+) rows?$")]
async fn cache_row_count(w: &mut DioramaWorld, n: u64) {
    let dio = w.dio.as_ref().expect("dio not created");
    let got = dio.cache().count().await.expect("cache count") as u64;
    assert_eq!(got, n, "expected {n} cache rows, got {got}");
}

#[then(regex = r#"^the master record "([^"]+)" has (\w+) "([^"]+)"$"#)]
async fn master_record_field(w: &mut DioramaWorld, id: String, field: String, expected: String) {
    let dio = w.dio.as_ref().expect("dio not created").clone();
    let id_for_dispatch = id.clone();
    let row = if w.backend == BackendKind::Sqlite {
        dispatch(async move { dio.master().get_value(&id_for_dispatch).await }).await
    } else {
        dio.master().get_value(&id_for_dispatch).await
    }
    .expect("master get_value")
    .unwrap_or_else(|| panic!("master has no record {id}"));
    let got = row
        .get(&field)
        .and_then(|v| match v {
            CborValue::Text(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert_eq!(
        got, expected,
        "master record {id}.{field}: want {expected}, got {got}"
    );
}

#[then(regex = r#"^the master record "([^"]+)" is absent$"#)]
async fn master_record_absent(w: &mut DioramaWorld, id: String) {
    let dio = w.dio.as_ref().expect("dio not created").clone();
    let id_for_dispatch = id.clone();
    let row = if w.backend == BackendKind::Sqlite {
        dispatch(async move { dio.master().get_value(&id_for_dispatch).await }).await
    } else {
        dio.master().get_value(&id_for_dispatch).await
    }
    .expect("master get_value");
    assert!(
        row.is_none(),
        "expected master record {id} to be absent, got {row:?}"
    );
}
