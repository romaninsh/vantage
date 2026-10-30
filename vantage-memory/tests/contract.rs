//! `vantage_vista::contract` over the in-memory store.

use std::time::Duration;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use tokio::time::{sleep, timeout};
use vantage_core::{Result, error};
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_types::Record;
use vantage_vista::contract::{TxFixture, TxTables};
use vantage_vista::{FilterOp, Transaction, Vista, VistaFactory, contract};

const PRODUCT: &str = r#"
name: product
id_column: id
columns:
  id: { type: string, flags: [id, orderable] }
  name: { type: string, flags: [title, searchable] }
  category: { type: string }
  origin: {}
"#;

const ORDER: &str = r#"
name: order
id_column: id
columns:
  id: { type: string, flags: [id] }
  product: { type: string }
references:
  product: { table: product, kind: has_one, foreign_key: product }
"#;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn record(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), text(v)))
        .collect()
}

/// `{ country: <country> }`, the map an `origin` cell holds.
fn origin(country: &str) -> CborValue {
    CborValue::Map(vec![(text("country"), text(country))])
}

/// Three products, two of them drinks, and one order pointing at a product.
/// Two products hold a map in `origin`, so the contract's dotted-path probe
/// checks that `origin.country` narrows.
fn factory() -> MemoryVistaFactory {
    let store = MemoryStore::new();
    let products = store.table("product");
    for (id, name, category, country) in [
        ("p1", "Tea", "drink", Some("India")),
        ("p2", "Cake", "food", None),
        ("p3", "Coffee", "drink", Some("Kenya")),
    ] {
        let mut row = record(&[("name", name), ("category", category)]);
        if let Some(country) = country {
            row.insert("origin".to_string(), origin(country));
        }
        products.upsert(id, row);
    }
    store
        .table("order")
        .upsert("o1", record(&[("product", "p1")]));
    MemoryVistaFactory::new(store)
}

#[tokio::test]
async fn memory_contract_advertised() {
    let f = factory();
    contract::check_advertised(&f.from_yaml(PRODUCT).unwrap()).await;
    contract::check_advertised(&f.from_yaml(ORDER).unwrap()).await;
    contract::check_advertised_writes(&f.from_yaml(PRODUCT).unwrap()).await;
}

const TX_A: &str = r#"
name: a
id_column: id
columns:
  id: { type: string, flags: [id] }
  n: { type: integer }
references:
  b: { table: b, kind: has_many, foreign_key: a_id }
"#;

const TX_B: &str = r#"
name: b
id_column: id
columns:
  id: { type: string, flags: [id] }
  a_id: { type: string }
  n: { type: integer }
"#;

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

/// The contract's `a` and `b` on `store`. The store checks no constraints,
/// so `a` is narrowed to `n >= 0`: an insert with a negative `n` doesn't
/// belong to the set and fails, as the check constraint does on SQL.
fn tables_on(store: MemoryStore) -> TxTables {
    let f = MemoryVistaFactory::new(store);
    let a = f.from_yaml(TX_A).unwrap();
    let a = a.with_condition("n", FilterOp::Gte, int(0)).unwrap();
    TxTables::new(a, f.from_yaml(TX_B).unwrap())
}

struct StoreTx;

#[async_trait]
impl TxFixture for StoreTx {
    async fn fresh(&self) -> TxTables {
        tables_on(MemoryStore::new())
    }

    async fn fresh_pool_of_one(&self) -> TxTables {
        self.fresh().await
    }

    fn detects_deadlocks(&self) -> bool {
        false
    }

    async fn fresh_other(&self) -> Option<TxTables> {
        Some(self.fresh().await)
    }
}

#[tokio::test]
async fn memory_transactions_hold_the_contract() {
    contract::check_transactions(&StoreTx).await
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn memory_atomic_holds_the_contract() {
    contract::check_atomic_rhai(&StoreTx).await
}

/// Reads `x.n` and writes it back plus one, in a transaction, pausing
/// between the read and the write.
async fn bump(a: &Vista) -> Result<()> {
    let tx = Transaction::new();
    let wrapped = tx.wrap(a);
    let row = wrapped
        .get_value("x")
        .await?
        .ok_or_else(|| error!("x is not found"))?;
    let n = match row.get("n") {
        Some(CborValue::Integer(n)) => i64::try_from(*n).unwrap_or_default(),
        other => return Err(error!(format!("x.n is {other:?}"))),
    };
    sleep(Duration::from_millis(20)).await;
    let next: Record<CborValue> = [("n".to_string(), int(n + 1))].into_iter().collect();
    wrapped.patch_value("x", &next).await?;
    tx.commit().await.map(drop)
}

/// Two blocks read-modify-write the same row at once. The second waits at
/// its read until the first commits, so neither update is lost.
#[tokio::test]
async fn concurrent_blocks_serialize() {
    let a = StoreTx.fresh().await.a;
    let zero: Record<CborValue> = [("n".to_string(), int(0))].into_iter().collect();
    a.insert_value("x", &zero).await.unwrap();
    let (one, two) = timeout(Duration::from_secs(2), async {
        tokio::join!(bump(&a), bump(&a))
    })
    .await
    .expect("the two blocks hung");
    one.unwrap();
    two.unwrap();
    let n = a.get_value("x").await.unwrap().unwrap().get("n").cloned();
    assert_eq!(n, Some(int(2)), "an update was lost");
}

/// The block patches `n` of y and w; meanwhile, outside it, y's `a_id` is
/// patched and w deleted. The commit merges its patch into y as it is now
/// and leaves w deleted.
#[tokio::test]
async fn an_outside_write_survives_a_commit() {
    let b = StoreTx.fresh().await.b;
    for id in ["y", "w"] {
        let row = rec(&[("a_id", CborValue::Text("x".into())), ("n", int(1))]);
        b.insert_value(id, &row).await.unwrap();
    }
    let tx = Transaction::new();
    let wrapped = tx.wrap(&b);
    for id in ["y", "w"] {
        wrapped
            .patch_value(id, &rec(&[("n", int(2))]))
            .await
            .unwrap();
    }
    b.patch_value("y", &rec(&[("a_id", CborValue::Text("q".into()))]))
        .await
        .unwrap();
    b.delete("w").await.unwrap();
    tx.commit().await.unwrap();

    let y = b.get_value("y").await.unwrap().unwrap();
    assert_eq!(y.get("a_id"), Some(&CborValue::Text("q".into())));
    assert_eq!(y.get("n"), Some(&int(2)));
    assert!(b.get_value("w").await.unwrap().is_none(), "w came back");
}

/// One row inserted with a generated id inside the block and one outside
/// it, while the block is open: both land, under different ids.
#[tokio::test]
async fn generated_ids_dont_collide_with_outside_inserts() {
    let b = StoreTx.fresh().await.b;
    let row = rec(&[("a_id", CborValue::Text("x".into())), ("n", int(1))]);
    let tx = Transaction::new();
    let inside = tx.wrap(&b).insert_return_id_value(&row).await.unwrap();
    let outside = b.insert_return_id_value(&row).await.unwrap();
    tx.commit().await.unwrap();

    assert_ne!(inside, outside);
    assert!(b.get_value(&inside).await.unwrap().is_some());
    assert!(b.get_value(&outside).await.unwrap().is_some());
}

/// The block inserts k and patches y; meanwhile an insert outside it takes
/// k. The commit fails with nothing applied, and the outside row stays.
#[tokio::test]
async fn an_insert_racing_an_outside_insert_of_the_same_id_fails_the_commit() {
    let b = StoreTx.fresh().await.b;
    let row = |a_id: &str| rec(&[("a_id", CborValue::Text(a_id.into())), ("n", int(1))]);
    b.insert_value("y", &row("x")).await.unwrap();
    let tx = Transaction::new();
    let wrapped = tx.wrap(&b);
    wrapped
        .patch_value("y", &rec(&[("n", int(2))]))
        .await
        .unwrap();
    wrapped.insert_value("k", &row("inside")).await.unwrap();
    b.insert_value("k", &row("outside")).await.unwrap();

    let err = tx.commit().await.unwrap_err().to_string();
    assert!(
        err.contains(r#""b" already has a row with id "k""#),
        "{err}"
    );
    let k = b.get_value("k").await.unwrap().unwrap();
    assert_eq!(k.get("a_id"), Some(&CborValue::Text("outside".into())));
    let y = b.get_value("y").await.unwrap().unwrap();
    assert_eq!(
        y.get("n"),
        Some(&int(1)),
        "the failed commit applied the patch of y"
    );
}

/// The block replaces y; meanwhile y is deleted outside it. The commit
/// skips the replace, as an SQL `UPDATE` that matches no row.
#[tokio::test]
async fn a_replaced_row_deleted_outside_stays_deleted() {
    let b = StoreTx.fresh().await.b;
    let row = rec(&[("a_id", CborValue::Text("x".into())), ("n", int(1))]);
    b.insert_value("y", &row).await.unwrap();
    let tx = Transaction::new();
    tx.wrap(&b).replace_value("y", &row).await.unwrap();
    b.delete("y").await.unwrap();
    tx.commit().await.unwrap();
    assert!(b.get_value("y").await.unwrap().is_none(), "y came back");
}

const BUSY: &str = "busy with another transaction";

/// Writes `first`, pauses, writes `second`, commits; rolls back on an error.
async fn across(first: &Vista, second: &Vista, pause: Duration) -> Result<()> {
    let tx = Transaction::new();
    let out = async {
        tx.wrap(first)
            .insert_value("x", &rec(&[("n", int(1))]))
            .await?;
        sleep(pause).await;
        tx.wrap(second)
            .insert_value("y", &rec(&[("n", int(1))]))
            .await?;
        tx.commit().await.map(drop)
    }
    .await;
    if out.is_err() {
        tx.rollback().await?;
    }
    out
}

/// Two blocks take two stores in opposite orders. The first to wait gives
/// up and rolls back, which frees its store for the other, which commits.
#[tokio::test]
async fn opposite_order_blocks_on_two_stores_dont_hang() {
    let wait = Duration::from_millis(300);
    let x = tables_on(MemoryStore::new().with_transaction_wait(wait)).a;
    let y = tables_on(MemoryStore::new().with_transaction_wait(wait)).a;
    let (one, two) = timeout(Duration::from_secs(4), async {
        tokio::join!(
            across(&x, &y, Duration::from_millis(20)),
            across(&y, &x, Duration::from_millis(100)),
        )
    })
    .await
    .expect("the two blocks hung");
    let busy = one.as_ref().err().map(|e| e.to_string());
    assert!(
        busy.as_deref().is_some_and(|e| e.contains(BUSY)),
        "the first block ends {one:?}, not `{BUSY}`"
    );
    two.unwrap();
    assert!(x.get_value("y").await.unwrap().is_some());
    assert!(y.get_value("x").await.unwrap().is_some());
    assert!(
        x.get_value("x").await.unwrap().is_none(),
        "the rolled-back write landed"
    );
}
