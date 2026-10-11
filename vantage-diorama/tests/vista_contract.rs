//! `vantage_vista::contract` over the Dio facade (`dio.vista()`), mastered by
//! in-memory SQLite so the master advertises paging, ordering, search and
//! operator filters for the facade to inherit.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Dio, Lens};
use vantage_sql::sqlite::SqliteDB;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::contract;

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
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .build()
            .expect("lens"),
    );
    lens.make_dio(master().await).await.unwrap()
}

#[tokio::test]
#[ignore = "v2 phase 3: facade advertises master flags it does not implement"]
async fn facade_contract_advertised() {
    contract::check_advertised(&dio().await.vista()).await;
}

#[tokio::test]
async fn facade_contract_clone() {
    let dio = dio().await;
    contract::check_clone(&dio.vista(), "category", CborValue::Text("drink".into())).await;
}
