//! Vista live-subscription integration: `Vista::watch()` over SurrealDB LIVE.
//!
//! Requires a running SurrealDB. Set `SURREALDB_URL` (defaults to
//! `cbor://root:root@localhost:8000/bakery/v2`). Runs in a fresh randomised
//! database so parallel runs don't collide.

#![cfg(feature = "vista")]

use std::error::Error;
use std::time::Duration;

use futures::StreamExt;
use uuid::Uuid;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::vista::factory::SurrealVistaFactory;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::VistaChange;

use surreal_client::{SurrealClient, SurrealConnection};

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn surreal_dsn(database: &str) -> String {
    let base = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());
    let (prefix, _) = base.rsplit_once('/').unwrap_or((&base, ""));
    format!("{}/{}", prefix, database)
}

async fn connect(database: &str) -> SurrealClient {
    SurrealConnection::dsn(surreal_dsn(database))
        .expect("Invalid SURREALDB_URL DSN")
        .connect()
        .await
        .expect("connect surreal")
}

fn product_table(db: SurrealDB, table_name: &str) -> Table<SurrealDB, EmptyEntity> {
    Table::<SurrealDB, EmptyEntity>::new(table_name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<i64>("stock")
}

async fn next_change(stream: &mut vantage_vista::VistaChangeStream) -> VistaChange {
    tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("timed out waiting for a vista change")
        .expect("watch stream closed")
        .expect("watch change was an error")
}

#[tokio::test]
async fn vista_watch_reports_insert_update_delete() -> TestResult {
    let database = format!("watch_{}", Uuid::new_v4().simple());
    let table_name = format!("bar_{}", Uuid::new_v4().simple());

    // A dedicated writer connection, separate from the one the vista watches —
    // proving updates flow over the database, not in-process.
    let writer = connect(&database).await;
    writer
        .query(&format!("DEFINE TABLE {table_name} SCHEMALESS"), None)
        .await?;

    let db = SurrealDB::new(connect(&database).await);
    let vista =
        SurrealVistaFactory::new(db.clone()).from_table(product_table(db.clone(), &table_name))?;
    assert!(
        vista.can_watch(),
        "surreal from_table vista should be watchable"
    );

    let mut stream = vista.watch().await?;

    // CREATE
    writer
        .query(
            &format!("CREATE {table_name}:negroni SET name = 'Negroni', price = 1200, stock = 5"),
            None,
        )
        .await?;
    match next_change(&mut stream).await {
        VistaChange::Inserted { id, value } => {
            assert!(id.ends_with(":negroni"), "unexpected id: {id}");
            let name = value
                .get("name")
                .map(|v| format!("{v:?}"))
                .unwrap_or_default();
            assert!(name.contains("Negroni"), "record should carry name: {name}");
        }
        other => panic!("expected Inserted, got {other:?}"),
    }

    // UPDATE
    writer
        .query(&format!("UPDATE {table_name}:negroni SET stock = 4"), None)
        .await?;
    match next_change(&mut stream).await {
        VistaChange::Updated { id, .. } => assert!(id.ends_with(":negroni")),
        other => panic!("expected Updated, got {other:?}"),
    }

    // DELETE
    writer
        .query(&format!("DELETE {table_name}:negroni"), None)
        .await?;
    match next_change(&mut stream).await {
        VistaChange::Deleted { id } => assert!(id.ends_with(":negroni")),
        other => panic!("expected Deleted, got {other:?}"),
    }

    Ok(())
}
