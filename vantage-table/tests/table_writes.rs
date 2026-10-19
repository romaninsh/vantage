#![cfg(feature = "mocks")]

//! The write pipeline of `Table`: set confinement, invariants, lifecycle hooks,
//! generated ids and audit timestamps. Expected outcomes come from the write
//! contract on `WritableValueSet` and the record-lifecycle chapter of the book.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use vantage_dataset::prelude::*;
use vantage_table::prelude::*;
use vantage_table::table::{
    AfterCx, AfterFn, BeforeCx, BeforeDeleteFn, BeforeFn, DeleteAllCx, DeleteAllFn, DeleteCx,
    SelectCx, after_hook, before_delete_hook, before_hook, delete_all_hook, select_hook,
};
use vantage_table::write_context::WriteContext;
use vantage_types::{EmptyEntity, Record};

type MockTable = Table<MockTableSource, EmptyEntity>;
type Log = Arc<Mutex<Vec<String>>>;

fn rec(value: Value) -> Record<Value> {
    Record::from(value)
}

async fn empty() -> MockTable {
    let src = MockTableSource::new().with_data("t", vec![]).await;
    MockTable::new("t", src)
}

async fn with_rows(rows: Vec<Value>) -> MockTable {
    let src = MockTableSource::new().with_data("t", rows).await;
    MockTable::new("t", src).with_id_column("id")
}

/// `all` holds `in1` (parent p1) and `out1` (parent p2); `set` is `all` narrowed
/// by `parent = "p1"`.
async fn sets() -> (MockTable, MockTable) {
    let all = with_rows(vec![
        json!({"id": "in1", "name": "a", "parent": "p1"}),
        json!({"id": "out1", "name": "b", "parent": "p2"}),
    ])
    .await;
    let condition = all
        .data_source()
        .eq_value_condition("parent", json!("p1"))
        .unwrap();
    let set = all.clone().with_condition(condition);
    (all, set)
}

fn recording_before(label: &'static str, log: Log) -> BeforeFn<MockTableSource> {
    before_hook(move |_cx: BeforeCx<'_, MockTableSource>| {
        let log = log.clone();
        Box::pin(async move {
            log.lock().unwrap().push(label.to_string());
            Ok(())
        })
    })
}

fn setting(column: &'static str, value: &'static str) -> BeforeFn<MockTableSource> {
    before_hook(move |cx: BeforeCx<'_, MockTableSource>| {
        cx.record.insert(column.to_string(), json!(value));
        Box::pin(async move { Ok(()) })
    })
}

fn failing_before() -> BeforeFn<MockTableSource> {
    before_hook(|_cx: BeforeCx<'_, MockTableSource>| {
        Box::pin(async move { Err(vantage_core::error!("rejected by hook")) })
    })
}

fn recording_after(label: &'static str, log: Log) -> AfterFn<MockTableSource> {
    after_hook(move |cx: AfterCx<'_, MockTableSource>| {
        let log = log.clone();
        let entry = format!("{label}:{}", cx.id);
        Box::pin(async move {
            log.lock().unwrap().push(entry);
            Ok(())
        })
    })
}

fn failing_after() -> AfterFn<MockTableSource> {
    after_hook(|_cx: AfterCx<'_, MockTableSource>| {
        Box::pin(async move { Err(vantage_core::error!("after-hook failed")) })
    })
}

fn veto() -> BeforeDeleteFn<MockTableSource> {
    before_delete_hook(|_cx: DeleteCx<'_, MockTableSource>| {
        Box::pin(async move { Err(vantage_core::error!("deletion not allowed")) })
    })
}

fn soft_delete() -> BeforeDeleteFn<MockTableSource> {
    before_delete_hook(|cx: DeleteCx<'_, MockTableSource>| {
        Box::pin(async move {
            let mut patch = Record::new();
            patch.insert("deleted".to_string(), json!("2026-01-01"));
            cx.table.patch_value(cx.id.clone(), &patch).await?;
            Ok(HookReturn::Handled)
        })
    })
}

fn seeing_former(seen: Arc<Mutex<Vec<Value>>>) -> BeforeDeleteFn<MockTableSource> {
    before_delete_hook(move |cx: DeleteCx<'_, MockTableSource>| {
        let seen = seen.clone();
        let former = cx.record["name"].clone();
        Box::pin(async move {
            seen.lock().unwrap().push(former);
            Ok(HookReturn::Proceed)
        })
    })
}

fn counting_after(calls: Arc<AtomicUsize>) -> AfterFn<MockTableSource> {
    after_hook(move |_cx: AfterCx<'_, MockTableSource>| {
        let calls = calls.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    })
}

// --- the set: confined writes -------------------------------------------------

#[tokio::test]
async fn delete_in_the_set_removes_the_row_and_a_retry_succeeds() {
    let (all, set) = sets().await;
    set.delete("in1").await.unwrap();
    assert!(all.get_value("in1").await.unwrap().is_none());
    set.delete("in1").await.unwrap();
}

#[tokio::test]
async fn delete_outside_the_set_reports_ok_and_leaves_the_row() {
    let (all, set) = sets().await;
    set.delete("out1").await.unwrap();
    assert!(all.get_value("out1").await.unwrap().is_some());
}

#[tokio::test]
async fn delete_of_a_missing_row_reports_ok() {
    let (_, set) = sets().await;
    set.delete("ghost").await.unwrap();
}

#[tokio::test]
async fn patch_that_nulls_the_sets_column_is_a_conflict() {
    let (_, set) = sets().await;
    let err = set
        .patch_value("in1", &rec(json!({"parent": null})))
        .await
        .unwrap_err();
    assert!(err.is_conflict(), "{err}");
}

#[tokio::test]
async fn patch_naming_the_sets_own_value_is_accepted() {
    let (_, set) = sets().await;
    let patched = set
        .patch_value("in1", &rec(json!({"parent": "p1", "name": "q"})))
        .await
        .unwrap();
    assert_eq!(patched["name"], json!("q"));
}

#[tokio::test]
async fn replace_in_the_set_overwrites_the_row_and_fills_the_sets_column() {
    let (all, set) = sets().await;
    set.replace_value("in1", &rec(json!({"name": "r"})))
        .await
        .unwrap();
    let row = all.get_value("in1").await.unwrap().unwrap();
    assert_eq!(row["name"], json!("r"));
    assert_eq!(row["parent"], json!("p1"));
}

#[tokio::test]
async fn replace_drops_attributes_the_new_record_lacks() {
    let all = with_rows(vec![json!({"id": "1", "name": "a", "age": 3})]).await;
    all.replace_value("1", &rec(json!({"name": "b"})))
        .await
        .unwrap();
    let row = all.get_value("1").await.unwrap().unwrap();
    assert_eq!(row["name"], json!("b"));
    assert!(row.get("age").is_none());
}

#[tokio::test]
async fn replace_of_an_id_held_outside_the_set_is_a_conflict_and_leaves_the_row() {
    let (all, set) = sets().await;
    let err = set
        .replace_value("out1", &rec(json!({"name": "r"})))
        .await
        .unwrap_err();
    assert!(err.is_conflict(), "{err}");
    assert_eq!(
        all.get_value("out1").await.unwrap().unwrap()["parent"],
        json!("p2")
    );
}

#[tokio::test]
async fn replace_that_would_leave_the_set_is_a_conflict() {
    let (_, set) = sets().await;
    let err = set
        .replace_value("in1", &rec(json!({"name": "r", "parent": "p2"})))
        .await
        .unwrap_err();
    assert!(err.is_conflict(), "{err}");
}

#[tokio::test]
async fn replace_creates_a_missing_row_that_fits_the_set() {
    let (all, set) = sets().await;
    set.replace_value("new3", &rec(json!({"name": "r"})))
        .await
        .unwrap();
    assert_eq!(
        all.get_value("new3").await.unwrap().unwrap()["parent"],
        json!("p1")
    );
}

#[tokio::test]
async fn an_unnarrowed_table_writes_anywhere() {
    let all = empty().await;
    all.delete("ghost").await.unwrap();
    all.insert_value("k1", &rec(json!({"name": "a"})))
        .await
        .unwrap();
    let again = all
        .insert_value("k1", &rec(json!({"name": "b"})))
        .await
        .unwrap();
    assert_eq!(again["name"], json!("a"));
    all.replace_value("k2", &rec(json!({"name": "c"})))
        .await
        .unwrap();
    assert!(all.get_value("k2").await.unwrap().is_some());
    let err = all
        .patch_value("ghost", &rec(json!({"name": "x"})))
        .await
        .unwrap_err();
    assert!(err.is_not_found(), "{err}");
}

#[tokio::test]
async fn conditioned_table_refuses_writes_the_source_cannot_confine() {
    let src = MockTableSource::new()
        .without_confined_writes()
        .with_data("t", vec![json!({"id": "1", "parent": "p1"})])
        .await;
    let all = MockTable::new("t", src.clone());
    let set = all
        .clone()
        .with_condition(src.eq_value_condition("parent", json!("p1")).unwrap());

    let r = rec(json!({"name": "x"}));
    assert!(
        set.insert_value("2", &r)
            .await
            .unwrap_err()
            .is_unsupported()
    );
    assert!(
        set.replace_value("1", &r)
            .await
            .unwrap_err()
            .is_unsupported()
    );
    assert!(set.patch_value("1", &r).await.unwrap_err().is_unsupported());
    assert!(set.delete("1").await.unwrap_err().is_unsupported());
    assert!(
        set.insert_return_id_value(&r)
            .await
            .unwrap_err()
            .is_unsupported()
    );
    all.delete("1").await.unwrap();
}

#[tokio::test]
async fn a_condition_the_source_cannot_evaluate_refuses_by_id_writes() {
    let all = with_rows(vec![json!({"id": "1", "price": 5})]).await;
    let set = all.with_condition(vantage_expressions::expr_any!("price > {}", 10));
    let r = rec(json!({"price": 20}));
    assert!(
        set.insert_value("2", &r)
            .await
            .unwrap_err()
            .is_unsupported()
    );
    assert!(set.patch_value("1", &r).await.unwrap_err().is_unsupported());
    assert!(set.delete("1").await.unwrap_err().is_unsupported());
}

// --- invariants ---------------------------------------------------------------

async fn scoped() -> MockTable {
    empty().await.with_invariant("parent_id", json!("p1"))
}

#[tokio::test]
async fn insert_fills_an_absent_invariant_column() {
    let table = scoped().await;
    let id = table
        .insert_return_id_value(&rec(json!({"name": "absent"})))
        .await
        .unwrap();
    assert_eq!(
        table.get_value(id).await.unwrap().unwrap()["parent_id"],
        json!("p1")
    );
}

#[tokio::test]
async fn insert_fills_a_null_invariant_column() {
    let table = scoped().await;
    let id = table
        .insert_return_id_value(&rec(json!({"name": "null", "parent_id": null})))
        .await
        .unwrap();
    assert_eq!(
        table.get_value(id).await.unwrap().unwrap()["parent_id"],
        json!("p1")
    );
}

#[tokio::test]
async fn insert_keeps_an_equal_invariant_value() {
    let table = scoped().await;
    let id = table
        .insert_return_id_value(&rec(json!({"name": "match", "parent_id": "p1"})))
        .await
        .unwrap();
    assert_eq!(
        table.get_value(id).await.unwrap().unwrap()["parent_id"],
        json!("p1")
    );
}

#[tokio::test]
async fn insert_with_a_conflicting_invariant_value_is_rejected() {
    let table = scoped().await;
    let err = table
        .insert_return_id_value(&rec(json!({"name": "wrong", "parent_id": "other"})))
        .await
        .unwrap_err();
    assert!(err.is_conflict(), "{err}");
    assert!(table.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_later_invariant_for_the_same_column_overwrites_the_earlier_one() {
    let table = scoped().await.with_invariant("parent_id", json!("p9"));
    assert_eq!(table.invariants().get("parent_id"), Some(&json!("p9")));
    assert_eq!(table.invariants().len(), 1);
}

#[tokio::test]
async fn with_id_narrows_to_the_row_and_makes_inserts_conform_to_the_id() {
    let base = with_rows(vec![
        json!({"id": "1", "name": "a"}),
        json!({"id": "2", "name": "b"}),
    ])
    .await;
    let one = base.clone().with_id(json!("1")).unwrap();
    assert_eq!(one.invariants().get("id"), Some(&json!("1")));
    let rows = one.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["1"]);

    let patched = one
        .patch_value("1", &rec(json!({"name": "b2"})))
        .await
        .unwrap();
    assert_eq!(patched["name"], json!("b2"));
}

#[tokio::test]
async fn with_id_without_an_id_field_is_an_error() {
    assert!(empty().await.with_id(json!("1")).is_err());
}

// --- lifecycle hooks ----------------------------------------------------------

#[tokio::test]
async fn before_insert_hook_mutates_the_record_before_it_is_written() {
    let table = empty().await.with_hook(Hook::BeforeInsert(
        Phase::Populate,
        setting("created", "yes"),
    ));
    let id = table
        .insert_return_id_value(&rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert_eq!(
        table.get_value(id).await.unwrap().unwrap()["created"],
        json!("yes")
    );
}

#[tokio::test]
async fn before_insert_hook_runs_for_insert_value_too() {
    let table = empty().await.with_hook(Hook::BeforeInsert(
        Phase::Populate,
        setting("created", "yes"),
    ));
    let stored = table
        .insert_value("1", &rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert_eq!(stored["created"], json!("yes"));
}

#[tokio::test]
async fn before_insert_hook_does_not_run_on_replace_or_patch() {
    let table = with_rows(vec![json!({"id": "1", "name": "a"})])
        .await
        .with_hook(Hook::BeforeInsert(
            Phase::Populate,
            setting("created", "yes"),
        ));
    table
        .patch_value("1", &rec(json!({"name": "b"})))
        .await
        .unwrap();
    table
        .replace_value("1", &rec(json!({"name": "c"})))
        .await
        .unwrap();
    assert!(
        table
            .get_value("1")
            .await
            .unwrap()
            .unwrap()
            .get("created")
            .is_none()
    );
}

#[tokio::test]
async fn before_update_hook_runs_on_replace_and_patch_but_not_insert() {
    let table = with_rows(vec![json!({"id": "1", "name": "a"})])
        .await
        .with_hook(Hook::BeforeUpdate(
            Phase::Populate,
            setting("touched", "yes"),
        ));
    let patched = table
        .patch_value("1", &rec(json!({"name": "b"})))
        .await
        .unwrap();
    assert_eq!(patched["touched"], json!("yes"));
    let replaced = table
        .replace_value("1", &rec(json!({"name": "c"})))
        .await
        .unwrap();
    assert_eq!(replaced["touched"], json!("yes"));
    let inserted = table
        .insert_value("2", &rec(json!({"name": "d"})))
        .await
        .unwrap();
    assert!(inserted.get("touched").is_none());
}

#[tokio::test]
async fn before_save_hook_runs_on_insert_and_update() {
    let table = with_rows(vec![json!({"id": "1", "name": "a"})])
        .await
        .with_hook(Hook::BeforeSave(Phase::Populate, setting("saved", "yes")));
    let inserted = table
        .insert_value("2", &rec(json!({"name": "d"})))
        .await
        .unwrap();
    assert_eq!(inserted["saved"], json!("yes"));
    let patched = table
        .patch_value("1", &rec(json!({"name": "b"})))
        .await
        .unwrap();
    assert_eq!(patched["saved"], json!("yes"));
}

#[tokio::test]
async fn before_hooks_run_in_phase_order_then_registration_order() {
    let log: Log = Arc::default();
    let table = empty()
        .await
        .with_hook(Hook::BeforeInsert(
            Phase::Validate,
            recording_before("validate", log.clone()),
        ))
        .with_hook(Hook::BeforeInsert(
            Phase::Populate,
            recording_before("populate-1", log.clone()),
        ))
        .with_hook(Hook::BeforeInsert(
            Phase::Normalize,
            recording_before("normalize", log.clone()),
        ))
        .with_hook(Hook::BeforeInsert(
            Phase::Populate,
            recording_before("populate-2", log.clone()),
        ));
    table
        .insert_return_id_value(&rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert_eq!(
        *log.lock().unwrap(),
        ["normalize", "populate-1", "populate-2", "validate"]
    );
}

#[test]
fn phases_order_normalize_then_populate_then_validate() {
    assert!(Phase::Normalize < Phase::Populate);
    assert!(Phase::Populate < Phase::Validate);
}

#[tokio::test]
async fn an_erroring_before_hook_cancels_the_write() {
    let table = empty()
        .await
        .with_hook(Hook::BeforeInsert(Phase::Populate, failing_before()));
    assert!(
        table
            .insert_value("1", &rec(json!({"name": "a"})))
            .await
            .is_err()
    );
    assert!(table.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn before_hooks_run_ahead_of_invariant_enforcement() {
    let table = scoped().await.with_hook(Hook::BeforeInsert(
        Phase::Populate,
        setting("parent_id", "other"),
    ));
    let err = table
        .insert_return_id_value(&rec(json!({"name": "a"})))
        .await
        .unwrap_err();
    assert!(err.is_conflict(), "{err}");
}

#[tokio::test]
async fn before_delete_hook_error_vetoes_the_delete() {
    let table = with_rows(vec![json!({"id": "1", "name": "a"})])
        .await
        .with_hook(Hook::BeforeDelete(veto()));
    assert!(table.delete("1").await.is_err());
    assert!(table.get_value("1").await.unwrap().is_some());
}

#[tokio::test]
async fn before_delete_handled_skips_the_real_delete_and_reports_success() {
    let table = with_rows(vec![json!({"id": "1", "name": "a"})])
        .await
        .with_hook(Hook::BeforeDelete(soft_delete()));
    table.delete("1").await.unwrap();
    let row = table.get_value("1").await.unwrap().unwrap();
    assert_eq!(row["deleted"], json!("2026-01-01"));
}

#[tokio::test]
async fn before_delete_hook_sees_the_former_row() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let table = with_rows(vec![json!({"id": "1", "name": "a"})])
        .await
        .with_hook(Hook::BeforeDelete(seeing_former(seen.clone())));
    table.delete("1").await.unwrap();
    assert_eq!(*seen.lock().unwrap(), [json!("a")]);
    assert!(table.get_value("1").await.unwrap().is_none());
}

#[tokio::test]
async fn after_insert_after_update_and_after_delete_hooks_fire_with_the_id() {
    let log: Log = Arc::default();
    let table = with_rows(vec![json!({"id": "1", "name": "a"})])
        .await
        .with_hook(Hook::AfterInsert(recording_after("ins", log.clone())))
        .with_hook(Hook::AfterUpdate(recording_after("upd", log.clone())))
        .with_hook(Hook::AfterDelete(recording_after("del", log.clone())));
    table
        .insert_value("2", &rec(json!({"name": "b"})))
        .await
        .unwrap();
    table
        .patch_value("1", &rec(json!({"name": "c"})))
        .await
        .unwrap();
    table
        .replace_value("1", &rec(json!({"name": "d"})))
        .await
        .unwrap();
    table.delete("2").await.unwrap();
    assert_eq!(*log.lock().unwrap(), ["ins:2", "upd:1", "upd:1", "del:2"]);
}

#[tokio::test]
async fn after_save_hook_fires_on_insert_and_update() {
    let log: Log = Arc::default();
    let table = with_rows(vec![json!({"id": "1", "name": "a"})])
        .await
        .with_hook(Hook::AfterSave(recording_after("save", log.clone())));
    table
        .insert_value("2", &rec(json!({"name": "b"})))
        .await
        .unwrap();
    table
        .patch_value("1", &rec(json!({"name": "c"})))
        .await
        .unwrap();
    assert_eq!(*log.lock().unwrap(), ["save:2", "save:1"]);
}

fn counting_delete_all(calls: Arc<AtomicUsize>) -> DeleteAllFn<MockTableSource> {
    delete_all_hook(move |_cx: DeleteAllCx<'_, MockTableSource>| {
        let calls = calls.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    })
}

#[tokio::test]
async fn delete_all_runs_its_hooks_once_and_never_the_row_hooks() {
    let row_calls = Arc::new(AtomicUsize::new(0));
    let before_all = Arc::new(AtomicUsize::new(0));
    let after_all = Arc::new(AtomicUsize::new(0));
    let table = with_rows(vec![
        json!({"id": "1", "name": "a"}),
        json!({"id": "2", "name": "b"}),
        json!({"id": "3", "name": "c"}),
    ])
    .await
    .with_hook(Hook::AfterDelete(counting_after(row_calls.clone())))
    .with_hook(Hook::BeforeDelete(veto()))
    .with_hook(Hook::BeforeDeleteAll(counting_delete_all(
        before_all.clone(),
    )))
    .with_hook(Hook::AfterDeleteAll(counting_delete_all(after_all.clone())));
    table.delete_all().await.unwrap();
    assert_eq!(row_calls.load(Ordering::SeqCst), 0);
    assert_eq!(before_all.load(Ordering::SeqCst), 1);
    assert_eq!(after_all.load(Ordering::SeqCst), 1);
    assert!(table.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn before_delete_all_error_cancels() {
    let table = with_rows(vec![json!({"id": "1", "name": "a"})])
        .await
        .with_hook(Hook::BeforeDeleteAll(delete_all_hook(
            |_cx: DeleteAllCx<'_, MockTableSource>| {
                Box::pin(async move { Err(vantage_core::error!("not allowed")) })
            },
        )));
    assert!(table.delete_all().await.is_err());
    assert_eq!(table.list_values().await.unwrap().len(), 1);
}

#[tokio::test]
async fn delete_all_hook_sees_the_set_conditions() {
    let seen = Arc::new(Mutex::new(0usize));
    let (_all, set) = sets().await;
    let s = seen.clone();
    let set = set.with_hook(Hook::BeforeDeleteAll(delete_all_hook(
        move |cx: DeleteAllCx<'_, MockTableSource>| {
            *s.lock().unwrap() = cx.table.conditions().count();
            Box::pin(async move { Ok(()) })
        },
    )));
    set.delete_all().await.unwrap();
    assert_eq!(*seen.lock().unwrap(), 1);
}

#[tokio::test]
async fn delete_outside_the_set_runs_no_hooks_and_deletes_nothing() {
    let (all, set) = sets().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let set = set.with_hook(Hook::AfterDelete(counting_after(calls.clone())));

    set.delete("out1").await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(all.get_value("out1").await.unwrap().is_some());

    set.delete("in1").await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    set.delete("in1").await.unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a missing row runs no hooks"
    );
}

#[tokio::test]
async fn a_failing_after_hook_surfaces_an_error_but_the_write_stays_committed() {
    let table = empty().await.with_hook(Hook::AfterInsert(failing_after()));
    assert!(
        table
            .insert_value("1", &rec(json!({"name": "a"})))
            .await
            .is_err()
    );
    assert!(table.get_value("1").await.unwrap().is_some());
}

#[tokio::test]
async fn prepare_insert_runs_hooks_and_invariants_without_writing() {
    let table = scoped().await.with_hook(Hook::BeforeInsert(
        Phase::Populate,
        setting("created", "yes"),
    ));
    let prepared = table
        .prepare_insert(&rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert_eq!(prepared["created"], json!("yes"));
    assert_eq!(prepared["parent_id"], json!("p1"));
    assert!(table.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn prepare_insert_refuses_a_table_whose_source_cannot_confine_writes() {
    let src = MockTableSource::new().without_confined_writes();
    let condition = src.eq_value_condition("parent", json!("p1")).unwrap();
    let set = MockTable::new("t", src).with_condition(condition);
    let err = set
        .prepare_insert(&rec(json!({"name": "a"})))
        .await
        .unwrap_err();
    assert!(err.is_unsupported(), "{err}");
}

#[tokio::test]
async fn finish_insert_runs_the_after_insert_hooks() {
    let log: Log = Arc::default();
    let table = empty()
        .await
        .with_hook(Hook::AfterInsert(recording_after("ins", log.clone())));
    table
        .finish_insert(&"7".to_string(), &rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert_eq!(*log.lock().unwrap(), ["ins:7"]);
}

// --- generated ids ------------------------------------------------------------

#[test]
fn uuid_generators_make_fresh_hyphenated_uuids_of_their_version() {
    let v7 = IdGenerator::UuidV7;
    let (a, b) = (v7.generate(), v7.generate());
    assert_ne!(a, b);
    assert_eq!(a.len(), 36);
    assert_eq!(a.as_bytes()[14], b'7');

    let v4 = IdGenerator::UuidV4;
    let c = v4.generate();
    assert_eq!(c.len(), 36);
    assert_eq!(c.as_bytes()[14], b'4');
}

#[test]
fn uuid_v7_ids_sort_by_creation_time() {
    let ids: Vec<String> = (0..3)
        .map(|_| {
            let id = IdGenerator::UuidV7.generate();
            std::thread::sleep(std::time::Duration::from_millis(2));
            id
        })
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn custom_generator_calls_the_closure() {
    assert_eq!(
        IdGenerator::custom(|| "made-up".into()).generate(),
        "made-up"
    );
}

#[tokio::test]
async fn with_generated_id_fills_a_missing_id_on_insert() {
    let table = empty().await.with_generated_id(IdGenerator::UuidV7);
    let id = table
        .insert_return_id_value(&rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert_eq!(id.len(), 36);
    assert_eq!(
        table.get_value(id).await.unwrap().unwrap()["name"],
        json!("a")
    );
}

#[tokio::test]
async fn with_generated_id_fills_a_null_id() {
    let table = empty().await.with_generated_id(IdGenerator::UuidV4);
    let id = table
        .insert_return_id_value(&rec(json!({"id": null, "name": "a"})))
        .await
        .unwrap();
    assert_eq!(id.len(), 36);
}

#[tokio::test]
async fn with_generated_id_keeps_a_caller_supplied_id() {
    let table = empty().await.with_generated_id(IdGenerator::UuidV7);
    let id = table
        .insert_return_id_value(&rec(json!({"id": "fixed-1", "name": "a"})))
        .await
        .unwrap();
    assert_eq!(id, "fixed-1");
}

#[tokio::test]
async fn with_generated_id_uses_the_custom_generator() {
    let table = empty()
        .await
        .with_generated_id(IdGenerator::custom(|| "made-up".into()));
    let id = table
        .insert_return_id_value(&rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert_eq!(id, "made-up");
}

#[tokio::test]
async fn add_generated_id_is_the_mutable_form() {
    let mut table = empty().await;
    table.add_generated_id(IdGenerator::custom(|| "made-up".into()));
    let id = table
        .insert_return_id_value(&rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert_eq!(id, "made-up");
}

#[tokio::test]
async fn generated_ids_never_touch_patch_or_replace() {
    let table = with_rows(vec![json!({"id": "1", "name": "a"})])
        .await
        .with_generated_id(IdGenerator::custom(|| "made-up".into()));
    table
        .patch_value("1", &rec(json!({"name": "b"})))
        .await
        .unwrap();
    table
        .replace_value("1", &rec(json!({"name": "c"})))
        .await
        .unwrap();
    let row = table.get_value("1").await.unwrap().unwrap();
    assert_eq!(row["name"], json!("c"));
    assert!(table.get_value("made-up").await.unwrap().is_none());
}

// --- audit timestamps ---------------------------------------------------------

fn is_rfc3339_utc_seconds(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 20
            && s.ends_with('Z')
            && s.as_bytes()[4] == b'-'
            && s.as_bytes()[10] == b'T'
            && s.as_bytes()[13] == b':'
    })
}

#[tokio::test]
async fn with_timestamps_stamps_created_and_updated_on_insert() {
    let table = empty().await.with_timestamps();
    let id = table
        .insert_return_id_value(&rec(json!({"id": "1", "name": "a"})))
        .await
        .unwrap();
    let row = table.get_value(id).await.unwrap().unwrap();
    assert!(is_rfc3339_utc_seconds(&row["created_at"]), "{row:?}");
    assert!(is_rfc3339_utc_seconds(&row["updated_at"]), "{row:?}");
}

#[tokio::test]
async fn with_timestamps_keeps_a_caller_supplied_created_at() {
    let table = empty().await.with_timestamps();
    let stored = table
        .insert_value(
            "1",
            &rec(json!({"name": "a", "created_at": "2000-01-01T00:00:00Z"})),
        )
        .await
        .unwrap();
    assert_eq!(stored["created_at"], json!("2000-01-01T00:00:00Z"));
    assert!(is_rfc3339_utc_seconds(&stored["updated_at"]));
}

#[tokio::test]
async fn with_timestamps_stamps_a_null_created_at() {
    let table = empty().await.with_timestamps();
    let stored = table
        .insert_value("1", &rec(json!({"name": "a", "created_at": null})))
        .await
        .unwrap();
    assert!(is_rfc3339_utc_seconds(&stored["created_at"]));
}

#[tokio::test]
async fn with_timestamps_updates_only_updated_at_on_patch() {
    let table = with_rows(vec![
        json!({"id": "1", "name": "a", "created_at": "2000-01-01T00:00:00Z"}),
    ])
    .await
    .with_timestamps();
    let patched = table
        .patch_value("1", &rec(json!({"name": "b"})))
        .await
        .unwrap();
    assert_eq!(patched["created_at"], json!("2000-01-01T00:00:00Z"));
    assert!(is_rfc3339_utc_seconds(&patched["updated_at"]));
    assert_eq!(patched["name"], json!("b"));
}

#[tokio::test]
async fn with_timestamps_rewrites_updated_at_on_every_write() {
    let table = with_rows(vec![
        json!({"id": "1", "name": "a", "updated_at": "2000-01-01T00:00:00Z"}),
    ])
    .await
    .with_timestamps();
    let replaced = table
        .replace_value(
            "1",
            &rec(json!({"name": "b", "updated_at": "2000-01-01T00:00:00Z"})),
        )
        .await
        .unwrap();
    assert_ne!(replaced["updated_at"], json!("2000-01-01T00:00:00Z"));
}

#[tokio::test]
async fn with_audit_stamps_the_named_columns() {
    let table = empty()
        .await
        .with_audit(Timestamps::new().created("inserted").updated("touched"));
    let stored = table
        .insert_value("1", &rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert!(is_rfc3339_utc_seconds(&stored["inserted"]));
    assert!(is_rfc3339_utc_seconds(&stored["touched"]));
    assert!(stored.get("created_at").is_none());
    assert!(stored.get("updated_at").is_none());
}

#[tokio::test]
async fn timestamps_default_to_created_at_and_updated_at() {
    let table = empty().await.with_audit(Timestamps::default());
    let stored = table
        .insert_value("1", &rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert!(stored.contains_key("created_at"));
    assert!(stored.contains_key("updated_at"));
}

// ---- before values on update ----

type Seen = Arc<Mutex<Vec<(Option<Value>, Value)>>>;

/// Records `(before[column], record[column])` for each after-update.
fn before_and_after(column: &'static str, seen: Seen) -> AfterFn<MockTableSource> {
    after_hook(move |cx: AfterCx<'_, MockTableSource>| {
        let pair = (
            cx.before.map(|b| b[column].clone()),
            cx.record[column].clone(),
        );
        let seen = seen.clone();
        Box::pin(async move {
            seen.lock().unwrap().push(pair);
            Ok(())
        })
    })
}

#[tokio::test]
async fn after_update_sees_former_and_written_row_on_replace() {
    let seen: Seen = Arc::default();
    let table = with_rows(vec![json!({"id": "1", "name": "a", "age": 1})])
        .await
        .with_hook(Hook::AfterUpdate(before_and_after("name", seen.clone())));
    table
        .replace_value("1", &rec(json!({"name": "b", "age": 1})))
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![(Some(json!("a")), json!("b"))]);
}

#[tokio::test]
async fn after_update_sees_former_and_written_row_on_patch() {
    let seen: Seen = Arc::default();
    let table = with_rows(vec![json!({"id": "1", "name": "a", "age": 1})])
        .await
        .with_hook(Hook::AfterUpdate(before_and_after("age", seen.clone())));
    table
        .patch_value("1", &rec(json!({"age": 2})))
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![(Some(json!(1)), json!(2))]);
}

fn counting_reads(calls: Arc<AtomicUsize>) -> Hook<MockTableSource> {
    Hook::BeforeSelect(select_hook(move |_cx: SelectCx<'_, MockTableSource>| {
        calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }))
}

/// A `BeforeSelect` counts the table's reads: an update reads the former
/// row only for an after-update hook.
#[tokio::test]
async fn update_reads_former_only_when_update_hooks_exist() {
    let row = || vec![json!({"id": "1", "name": "a", "age": 1})];
    let update = |table: MockTable| async move {
        table
            .replace_value("1", &rec(json!({"name": "b", "age": 1})))
            .await
            .unwrap();
        table
            .patch_value("1", &rec(json!({"age": 2})))
            .await
            .unwrap();
    };

    let reads = Arc::new(AtomicUsize::new(0));
    update(
        with_rows(row())
            .await
            .with_hook(counting_reads(reads.clone())),
    )
    .await;
    assert_eq!(reads.load(Ordering::SeqCst), 0, "no update hooks, no read");

    let reads = Arc::new(AtomicUsize::new(0));
    let seen: Seen = Arc::default();
    let hooked = with_rows(row())
        .await
        .with_hook(counting_reads(reads.clone()))
        .with_hook(Hook::AfterUpdate(before_and_after("age", seen)));
    update(hooked).await;
    assert_eq!(
        reads.load(Ordering::SeqCst),
        2,
        "one former-row read per update"
    );
}

#[tokio::test]
async fn after_save_on_insert_has_no_before() {
    let seen: Seen = Arc::default();
    let table = empty()
        .await
        .with_hook(Hook::AfterSave(before_and_after("name", seen.clone())));
    table
        .insert_value("1", &rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![(None, json!("a"))]);
}

#[tokio::test]
async fn patch_of_a_row_outside_the_set_runs_no_after_hook() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (all, set) = sets().await;
    let set = set.with_hook(Hook::AfterUpdate(counting_after(calls.clone())));
    let _ = set.patch_value("out1", &rec(json!({"name": "x"}))).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        all.get_value("out1").await.unwrap().unwrap()["name"],
        json!("b"),
        "the row outside the set is untouched"
    );
}

// ---- write context ----

fn principal(name: &str) -> ciborium::Value {
    ciborium::Value::Map(vec![(
        ciborium::Value::Text("name".into()),
        ciborium::Value::Text(name.into()),
    )])
}

fn group_seen_before(seen: Arc<Mutex<Vec<Option<String>>>>) -> BeforeFn<MockTableSource> {
    before_hook(move |cx: BeforeCx<'_, MockTableSource>| {
        let group = cx.context.and_then(|c| c.group.clone());
        let seen = seen.clone();
        Box::pin(async move {
            seen.lock().unwrap().push(group);
            Ok(())
        })
    })
}

fn group_seen_after(seen: Arc<Mutex<Vec<Option<String>>>>) -> AfterFn<MockTableSource> {
    after_hook(move |cx: AfterCx<'_, MockTableSource>| {
        let group = cx.context.and_then(|c| c.group.clone());
        let seen = seen.clone();
        Box::pin(async move {
            seen.lock().unwrap().push(group);
            Ok(())
        })
    })
}

#[tokio::test]
async fn write_context_reaches_before_and_after_hooks() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let table = empty()
        .await
        .with_hook(Hook::BeforeInsert(
            Phase::Populate,
            group_seen_before(seen.clone()),
        ))
        .with_hook(Hook::AfterInsert(group_seen_after(seen.clone())));
    let context = WriteContext::new()
        .with_principal(principal("ana"))
        .with_group("g1");
    context
        .scope(async { table.insert_value("1", &rec(json!({"name": "a"}))).await })
        .await
        .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        vec![Some("g1".to_string()), Some("g1".to_string())]
    );
}

#[tokio::test]
async fn no_context_outside_a_scope() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let table = empty()
        .await
        .with_hook(Hook::BeforeInsert(
            Phase::Populate,
            group_seen_before(seen.clone()),
        ))
        .with_hook(Hook::AfterInsert(group_seen_after(seen.clone())));
    table
        .insert_value("1", &rec(json!({"name": "a"})))
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![None, None]);
}

#[tokio::test]
async fn write_context_current_is_none_outside_a_scope() {
    assert!(WriteContext::current().is_none());
    let inside = WriteContext::new()
        .with_principal(principal("ana"))
        .scope(async { WriteContext::current() })
        .await;
    assert_eq!(inside.unwrap().principal, Some(principal("ana")));
}
