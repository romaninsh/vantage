//! Roll-up columns computed by `MemoryDB`: an investor's cash is the sum of
//! its ledger's amounts.

use ciborium::Value as CborValue;
use vantage_dataset::WritableValueSet;
use vantage_memory::prelude::*;
use vantage_memory::{MemoryDB, MemoryRollup, MemoryVistaFactory};
use vantage_table::prelude::*;
use vantage_table::table::RollupOp;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{FilterOp, Transaction};

fn over_ledger(column: &str, op: RollupOp, source: Option<&str>) -> MemoryRollup {
    let rollup = MemoryRollup::new(column, "txn", "investor", op);
    match source {
        Some(source) => rollup.with_source(source),
        None => rollup,
    }
}

fn investors(db: &MemoryDB) -> Table<MemoryDB, EmptyEntity> {
    Table::new("investor", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_memory_rollup(over_ledger("cash", RollupOp::Sum, Some("amount")))
}

fn ledger(db: &MemoryDB) -> Table<MemoryDB, EmptyEntity> {
    Table::new("txn", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("investor")
        .with_column_of::<String>("kind")
        .with_column_of::<i64>("amount")
}

fn named(name: &str) -> Record<AnyMemoryType> {
    [("name".to_string(), AnyMemoryType::from(name))]
        .into_iter()
        .collect()
}

async fn post(db: &MemoryDB, id: &str, investor: &str, kind: &str, amount: i64) {
    let t = ledger(db);
    let row: Record<AnyMemoryType> = [
        ("investor".to_string(), AnyMemoryType::from(investor)),
        ("kind".to_string(), AnyMemoryType::from(kind)),
        ("amount".to_string(), AnyMemoryType::from(amount)),
    ]
    .into_iter()
    .collect();
    db.insert_table_value(&t, &id.to_string(), &row)
        .await
        .unwrap();
}

/// `v1` holds 100 - 30, `v2` 50, `v3` nothing.
async fn books() -> MemoryDB {
    let db = MemoryDB::new();
    let t = investors(&db);
    for id in ["v1", "v2", "v3"] {
        db.insert_table_value(&t, &id.to_string(), &named(id))
            .await
            .unwrap();
    }
    post(&db, "t1", "v1", "deposit", 100).await;
    post(&db, "t2", "v1", "hold", -30).await;
    post(&db, "t3", "v2", "deposit", 50).await;
    db
}

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

async fn cell(t: &Table<MemoryDB, EmptyEntity>, id: &str, column: &str) -> CborValue {
    let row = t.data_source().get_table_value(t, &id.to_string()).await;
    row.unwrap().unwrap()[column].value().clone()
}

async fn ids(t: &Table<MemoryDB, EmptyEntity>) -> Vec<String> {
    let rows = t.data_source().list_table_values(t).await.unwrap();
    rows.keys().cloned().collect()
}

#[tokio::test]
async fn a_rollup_sums_its_children() {
    let db = books().await;
    let t = investors(&db);
    let rows = t.data_source().list_table_values(&t).await.unwrap();
    assert_eq!(rows["v1"]["cash"].value(), &int(70));
    assert_eq!(rows["v2"]["cash"].value(), &int(50));
    assert_eq!(cell(&t, "v1", "cash").await, int(70));
}

#[tokio::test]
async fn a_rollup_with_conditions_filters_children() {
    let db = books().await;
    let deposits = over_ledger("deposits", RollupOp::Sum, Some("amount"))
        .with_conditions([MemoryCondition::cmp("kind", FilterOp::Eq, "deposit")]);
    let t = investors(&db).with_memory_rollup(deposits);
    assert_eq!(cell(&t, "v1", "deposits").await, int(100));
    assert_eq!(cell(&t, "v1", "cash").await, int(70));
}

#[tokio::test]
async fn count_and_sum_over_no_children_are_zero() {
    let db = books().await;
    let t = investors(&db)
        .with_memory_rollup(over_ledger("moves", RollupOp::Count, None))
        .with_memory_rollup(over_ledger("largest", RollupOp::Max, Some("amount")));
    let rows = t.data_source().list_table_values(&t).await.unwrap();
    assert_eq!(rows["v3"]["cash"].value(), &int(0));
    assert_eq!(rows["v3"]["moves"].value(), &int(0));
    assert_eq!(rows["v3"]["largest"].value(), &CborValue::Null);
    assert_eq!(rows["v1"]["moves"].value(), &int(2));
    assert_eq!(rows["v1"]["largest"].value(), &int(100));
}

#[tokio::test]
async fn a_rollup_moves_after_a_child_insert() {
    let db = books().await;
    let t = investors(&db);
    post(&db, "t4", "v2", "hold", -20).await;
    assert_eq!(cell(&t, "v2", "cash").await, int(30));
}

#[tokio::test]
async fn a_rollup_sees_the_open_transaction() {
    let db = books().await;
    let tx = Transaction::new();
    let joined = db.in_transaction(&tx).unwrap();
    post(&joined, "t5", "v1", "hold", -70).await;

    assert_eq!(cell(&investors(&joined), "v1", "cash").await, int(0));
    assert_eq!(cell(&investors(&db), "v1", "cash").await, int(70));
    tx.commit().await.unwrap();
    assert_eq!(cell(&investors(&db), "v1", "cash").await, int(0));
}

#[tokio::test]
async fn a_written_rollup_value_is_ignored_on_read() {
    let db = books().await;
    let t = investors(&db);
    let mut fixture = named("v1");
    fixture.insert("cash".into(), AnyMemoryType::from(999_i64));
    db.replace_table_value(&t, &"v1".to_string(), &fixture)
        .await
        .unwrap();
    assert!(db.store().table("investor").get("v1").unwrap()["cash"] == int(999));
    assert_eq!(cell(&t, "v1", "cash").await, int(70));
}

#[tokio::test]
async fn a_delete_on_a_set_narrowed_by_a_rollup_deletes_the_row() {
    let db = books().await;
    let mut poor = investors(&db);
    poor.add_condition(poor["cash"].lt(60));
    let ds = poor.data_source();
    ds.delete_table_value(&poor, &"v2".to_string())
        .await
        .unwrap();
    ds.delete_table_value(&poor, &"v1".to_string())
        .await
        .unwrap();
    assert_eq!(db.store().table("investor").ids(), ["v1", "v3"]);
}

#[tokio::test]
async fn a_patch_returns_the_computed_rollup_not_the_stored_value() {
    let db = books().await;
    let mut stored = (*db.store().table("investor").get("v1").unwrap()).clone();
    stored.insert("cash".into(), int(100));
    db.store().table("investor").upsert("v1", stored);
    let t = investors(&db);
    let patched = t
        .data_source()
        .patch_table_value(&t, &"v1".to_string(), &named("Vera"))
        .await
        .unwrap();
    assert_eq!(patched["cash"].value(), &int(70));
}

#[tokio::test]
async fn a_new_parent_returns_zero_for_its_sum_rollup() {
    let db = books().await;
    let t = investors(&db);
    let ds = t.data_source();
    let inserted = ds
        .insert_table_value(&t, &"v4".to_string(), &named("New"))
        .await
        .unwrap();
    assert_eq!(inserted["cash"].value(), &int(0));
    let replaced = ds
        .replace_table_value(&t, &"v4".to_string(), &named("Newer"))
        .await
        .unwrap();
    assert_eq!(replaced["cash"].value(), &int(0));
}

fn vista_rows(pairs: &[(&str, &str)]) -> indexmap::IndexMap<String, Record<CborValue>> {
    pairs
        .iter()
        .map(|(id, name)| {
            let row = [("name".to_string(), CborValue::Text(name.to_string()))];
            (id.to_string(), row.into_iter().collect())
        })
        .collect()
}

#[tokio::test]
async fn an_import_checks_a_rollup_condition_on_the_filled_row() {
    let db = books().await;
    let poor = MemoryVistaFactory::new(db.store().clone())
        .from_table(investors(&db))
        .unwrap()
        .with_condition("cash", FilterOp::Lt, int(60))
        .unwrap();
    let inserted = poor.import_values(&vista_rows(&[("v4", "New")])).await;
    assert_eq!(inserted.unwrap(), 1);
}

#[tokio::test]
async fn an_equality_on_a_rollup_column_is_not_written() {
    let db = books().await;
    let broke = MemoryVistaFactory::new(db.store().clone())
        .from_table(investors(&db))
        .unwrap()
        .with_condition("cash", FilterOp::Eq, int(0))
        .unwrap();
    let rows = vista_rows(&[("v5", "Newer")]);
    broke.insert_value("v5", &rows["v5"]).await.unwrap();
    let stored = db.store().table("investor").get("v5").unwrap();
    assert!(stored.get("cash").is_none(), "{stored:?}");
}

#[tokio::test]
async fn a_condition_on_a_rollup_column_filters() {
    let db = books().await;
    let mut poor = investors(&db);
    poor.add_condition(poor["cash"].lt(60));
    assert_eq!(ids(&poor).await, ["v2", "v3"]);
    let ds = poor.data_source();
    assert_eq!(ds.get_table_count(&poor).await.unwrap(), 2);
    let v1 = ds.get_table_value(&poor, &"v1".to_string()).await.unwrap();
    assert!(v1.is_none());

    let mut richest = investors(&db);
    richest.add_order(richest["cash"].descending());
    richest.set_pagination(Some(Pagination::window(0, 2)));
    assert_eq!(ids(&richest).await, ["v1", "v2"]);
}

fn decimal(text: &str) -> CborValue {
    CborValue::Tag(10, Box::new(CborValue::Text(text.into())))
}

/// Investors `v1` and `v2` and the ledger `moves`, written to the store.
fn raw_books(moves: Vec<(&str, &str, CborValue)>) -> MemoryDB {
    let db = MemoryDB::new();
    for id in ["v1", "v2"] {
        let row: Record<CborValue> = [
            ("id".to_string(), CborValue::Text(id.into())),
            ("name".to_string(), CborValue::Text(id.into())),
        ]
        .into_iter()
        .collect();
        db.store().table("investor").upsert(id, row);
    }
    let store = db.store().table("txn");
    for (id, investor, amount) in moves {
        let row: Record<CborValue> = [
            ("id".to_string(), CborValue::Text(id.into())),
            ("investor".to_string(), CborValue::Text(investor.into())),
            ("amount".to_string(), amount),
        ]
        .into_iter()
        .collect();
        store.upsert(id, row);
    }
    db
}

/// `v1` holds two decimals and an integer, `v2` a decimal and a float.
#[tokio::test]
async fn a_rollup_sums_decimal_children() {
    let db = raw_books(vec![
        ("d1", "v1", decimal("10.25")),
        ("d2", "v1", decimal("0.50")),
        ("d3", "v1", int(4)),
        ("d4", "v2", decimal("1.5")),
        ("d5", "v2", CborValue::Float(2.0)),
    ]);
    let t = investors(&db)
        .with_memory_rollup(over_ledger("low", RollupOp::Min, Some("amount")))
        .with_memory_rollup(over_ledger("high", RollupOp::Max, Some("amount")))
        .with_memory_rollup(over_ledger("mean", RollupOp::Avg, Some("amount")));
    let rows = t.data_source().list_table_values(&t).await.unwrap();
    let v1 = |c: &str| rows["v1"][c].value().clone();
    assert_eq!(v1("cash"), decimal("14.75"));
    assert_eq!(v1("low"), decimal("0.50"));
    assert_eq!(v1("high"), decimal("10.25"));
    let CborValue::Tag(10, mean) = v1("mean") else {
        panic!("not a decimal: {:?}", v1("mean"));
    };
    assert!(matches!(*mean, CborValue::Text(ref s) if s.starts_with("4.916")));
    let v2 = |c: &str| rows["v2"][c].value().clone();
    assert_eq!(v2("cash"), CborValue::Float(3.5));
    assert_eq!(v2("low"), decimal("1.5"));
    assert_eq!(v2("high"), CborValue::Float(2.0));
    assert_eq!(v2("mean"), CborValue::Float(1.75));
}

/// `Decimal::MAX` plus a large integer overflows the decimal sum.
#[tokio::test]
async fn a_decimal_sum_past_the_decimal_range_is_a_float() {
    let max = "79228162514264337593543950335";
    let db = raw_books(vec![
        ("d1", "v1", decimal(max)),
        ("d2", "v1", CborValue::Integer(u64::MAX.into())),
    ]);
    let t = investors(&db);
    let CborValue::Float(cash) = cell(&t, "v1", "cash").await else {
        panic!("not a float");
    };
    assert!(cash > 7.9e28);
}
