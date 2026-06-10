//! YAML-vista reference traversal — has_many / has_one chains end-to-end
//! against a live SurrealDB. Demonstrates that many-to-many emerges from
//! chained traversal (bakery → clients → bakery) without any new YAML kind.
//!
//! Each test runs in a fresh randomised database so parallel runs don't
//! collide. Reads `SURREALDB_URL` (defaults to localhost:8000) for the host.

#![cfg(feature = "vista")]

use std::error::Error;
use std::sync::Arc;

use indexmap::IndexMap;
use surreal_client::SurrealConnection;
use uuid::Uuid;
use vantage_dataset::prelude::*;
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::vista::{SurrealSpecResolver, SurrealVistaFactory, SurrealVistaSpec};
use vantage_vista::VistaFactory;

type TestResult = std::result::Result<(), Box<dyn Error>>;

fn surreal_dsn(database: &str) -> String {
    let base = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());
    let (prefix, _) = base.rsplit_once('/').unwrap_or((&base, ""));
    format!("{}/{}", prefix, database)
}

async fn setup() -> (SurrealDB, String, String) {
    let database = format!("vista_refs_{}", Uuid::new_v4().simple());
    let client = SurrealConnection::dsn(surreal_dsn(&database))
        .expect("Invalid SURREALDB_URL DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    let db = SurrealDB::new(client);
    let bakery_table = format!("bakery_{}", Uuid::new_v4().simple());
    let client_table = format!("client_{}", Uuid::new_v4().simple());
    (db, bakery_table, client_table)
}

async fn seed_v2_like(
    db: &SurrealDB,
    bakery_table: &str,
    client_table: &str,
) -> std::result::Result<(), Box<dyn Error>> {
    db.execute(&surreal_expr!(&format!(
        "CREATE {}:hill_valley SET name = 'Hill Valley Bakery'",
        bakery_table
    )))
    .await?;
    db.execute(&surreal_expr!(&format!(
        "CREATE {}:marty SET name = 'Marty McFly', bakery = {}:hill_valley",
        client_table, bakery_table
    )))
    .await?;
    db.execute(&surreal_expr!(&format!(
        "CREATE {}:doc SET name = 'Doc Brown', bakery = {}:hill_valley",
        client_table, bakery_table
    )))
    .await?;
    db.execute(&surreal_expr!(&format!(
        "CREATE {}:biff SET name = 'Biff Tannen', bakery = {}:hill_valley",
        client_table, bakery_table
    )))
    .await?;
    Ok(())
}

fn registry_resolver(specs: Vec<(String, SurrealVistaSpec)>) -> SurrealSpecResolver {
    let map: IndexMap<String, SurrealVistaSpec> = specs.into_iter().collect();
    let map = Arc::new(map);
    Arc::new(move |name: &str| map.get(name).cloned())
}

fn build_specs(bakery_table: &str, client_table: &str) -> (SurrealVistaSpec, SurrealVistaSpec) {
    let bakery_yaml = format!(
        r#"
name: bakery
columns:
  id: {{ type: thing, flags: [id] }}
  name: {{ type: string, flags: [title] }}
surreal:
  table: {}
references:
  clients:
    table: client
    kind: has_many
    foreign_key: bakery
"#,
        bakery_table
    );
    let client_yaml = format!(
        r#"
name: client
columns:
  id: {{ type: thing, flags: [id] }}
  name: {{ type: string, flags: [title] }}
  bakery: {{ type: thing }}
surreal:
  table: {}
references:
  bakery:
    table: bakery
    kind: has_one
    foreign_key: bakery
"#,
        client_table
    );
    (
        serde_yaml_ng::from_str(&bakery_yaml).expect("bakery yaml"),
        serde_yaml_ng::from_str(&client_yaml).expect("client yaml"),
    )
}

#[tokio::test]
async fn has_many_traversal_lists_related_rows() -> TestResult {
    let (db, bakery_table, client_table) = setup().await;
    seed_v2_like(&db, &bakery_table, &client_table).await?;

    let (bakery_spec, client_spec) = build_specs(&bakery_table, &client_table);
    let resolver = registry_resolver(vec![
        ("bakery".into(), bakery_spec.clone()),
        ("client".into(), client_spec),
    ]);
    let factory = SurrealVistaFactory::new(db.clone()).with_resolver(resolver);

    let bakery = factory.build_from_spec(bakery_spec)?;
    let bakery_row = bakery.get_value("hill_valley").await?.expect("bakery row");

    let clients = bakery.get_ref("clients", &bakery_row)?;
    let rows = clients.list_values().await?;
    assert_eq!(rows.len(), 3, "hill_valley has 3 clients");

    let mut names: Vec<String> = rows
        .values()
        .filter_map(|r| match r.get("name") {
            Some(ciborium::Value::Text(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();
    names.sort();
    assert_eq!(names, vec!["Biff Tannen", "Doc Brown", "Marty McFly"]);

    Ok(())
}

#[tokio::test]
async fn has_one_traversal_returns_parent_row() -> TestResult {
    let (db, bakery_table, client_table) = setup().await;
    seed_v2_like(&db, &bakery_table, &client_table).await?;

    let (bakery_spec, client_spec) = build_specs(&bakery_table, &client_table);
    let resolver = registry_resolver(vec![
        ("bakery".into(), bakery_spec),
        ("client".into(), client_spec.clone()),
    ]);
    let factory = SurrealVistaFactory::new(db.clone()).with_resolver(resolver);

    let client = factory.build_from_spec(client_spec)?;
    let client_row = client.get_value("marty").await?.expect("client row");

    let bakery = client.get_ref("bakery", &client_row)?;
    let rows = bakery.list_values().await?;
    assert_eq!(rows.len(), 1, "marty belongs to one bakery");
    let row = rows.values().next().expect("bakery row");
    assert_eq!(
        row.get("name"),
        Some(&ciborium::Value::Text("Hill Valley Bakery".to_string()))
    );

    Ok(())
}

/// Many-to-many via chained traversal: client → bakery → clients.
/// "Other clients of the same bakery as Marty" — relational m2m without
/// any new YAML keyword.
#[tokio::test]
async fn many_to_many_via_chained_has_many_through_has_one() -> TestResult {
    let (db, bakery_table, client_table) = setup().await;
    seed_v2_like(&db, &bakery_table, &client_table).await?;

    let (bakery_spec, client_spec) = build_specs(&bakery_table, &client_table);
    let resolver = registry_resolver(vec![
        ("bakery".into(), bakery_spec),
        ("client".into(), client_spec.clone()),
    ]);
    let factory = SurrealVistaFactory::new(db.clone()).with_resolver(resolver);

    let client = factory.build_from_spec(client_spec)?;
    let marty = client.get_value("marty").await?.expect("marty");

    let bakery_for_marty = client.get_ref("bakery", &marty)?;
    let bakery_rows = bakery_for_marty.list_values().await?;
    assert_eq!(bakery_rows.len(), 1);
    let (_, bakery_row) = bakery_rows.into_iter().next().unwrap();

    let sibling_clients = bakery_for_marty.get_ref("clients", &bakery_row)?;
    let sibling_rows = sibling_clients.list_values().await?;
    assert_eq!(sibling_rows.len(), 3, "all 3 clients of marty's bakery");

    Ok(())
}
