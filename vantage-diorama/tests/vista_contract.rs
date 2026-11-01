//! `vantage_vista::contract` over the Dio facade (`dio.cached()`), mastered by
//! in-memory SQLite, a master that can page, order, search and filter. The
//! facade advertises only what it answers itself, whatever the master can do.

use ciborium::Value as CborValue;
use vantage_diorama::Dio;
use vantage_sql::sqlite::SqliteDB;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{Capability, contract};

mod support;

/// Products `p1`..`p3`, two of them in category `drink`.
async fn master() -> vantage_vista::Vista {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    for sql in [
        "CREATE TABLE product (id TEXT PRIMARY KEY, name TEXT, category TEXT)",
        "INSERT INTO product VALUES ('p1','Tea','drink'),('p2','Cake','food'),('p3','Coffee','drink')",
    ] {
        sqlx::query(sql).execute(db.pool()).await.unwrap();
    }
    let table = Table::<SqliteDB, EmptyEntity>::new("product", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("category");
    db.vista_factory().from_table(table).unwrap()
}

/// A Dio whose in-memory cache is filled from the master on start.
async fn dio() -> Dio {
    support::eager_dio(master().await).await
}

#[tokio::test]
async fn facade_contract_advertised() {
    contract::check_advertised(&dio().await.cached()).await;
}

#[tokio::test]
async fn facade_contract_advertised_writes() {
    contract::check_advertised_writes(&dio().await.cached()).await;
}

#[tokio::test]
async fn facade_contract_clone() {
    let dio = dio().await;
    contract::check_clone(&dio.cached(), "category", CborValue::Text("drink".into())).await;
}

/// The facade evaluates search and operator conditions itself, so it
/// advertises both; it advertises nothing it would refuse.
#[tokio::test]
async fn facade_capability_set() {
    let facade = dio().await.cached();
    let mut advertised: Vec<Capability> = facade.capabilities().iter().collect();
    advertised.sort_by_key(|c| format!("{c:?}"));
    let mut expected = vec![
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::DeleteAll,
        Capability::Subscribe,
        Capability::Search,
        Capability::FilterOps,
        Capability::Order,
        Capability::Window,
        Capability::ConfineWrites,
    ];
    expected.sort_by_key(|c| format!("{c:?}"));
    assert_eq!(advertised, expected);
}
