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
use vantage_surrealdb::vista::{SurrealVistaFactory, SurrealVistaSpec};
use vantage_vista::{ColumnFlag, SpecResolver, VistaFactory};

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

fn registry_resolver(
    specs: Vec<(String, SurrealVistaSpec)>,
) -> Arc<dyn SpecResolver<SurrealVistaSpec>> {
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

    let clients = bakery.get_ref("clients", Some(&bakery_row))?;
    assert!(clients.is_narrowed(), "the join is recorded on the target");
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

    let bakery = client.get_ref("bakery", Some(&client_row))?;
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

    let bakery_for_marty = client.get_ref("bakery", Some(&marty))?;
    let bakery_rows = bakery_for_marty.list_values().await?;
    assert_eq!(bakery_rows.len(), 1);
    let (_, bakery_row) = bakery_rows.into_iter().next().unwrap();

    let sibling_clients = bakery_for_marty.get_ref("clients", Some(&bakery_row))?;
    let sibling_rows = sibling_clients.list_values().await?;
    assert_eq!(sibling_rows.len(), 3, "all 3 clients of marty's bakery");

    Ok(())
}

/// Every driver answers an unknown relation with `unknown_relation`: a
/// not-found error.
#[tokio::test]
async fn unknown_relation_is_not_found() -> TestResult {
    let (db, bakery_table, client_table) = setup().await;
    let (bakery_spec, _) = build_specs(&bakery_table, &client_table);
    let bakery = SurrealVistaFactory::new(db).build_from_spec(bakery_spec)?;

    let err = bakery
        .get_ref("nope", None)
        .map(drop)
        .expect_err("no such relation");
    assert!(err.is_not_found(), "{err:?}");
    Ok(())
}

/// A dotted spec column through a declared relation (`bakery.name`) is an
/// implicit reference: each row carries the related value under the dotted
/// name, and the column is flagged calculated (read-only for consumers).
#[tokio::test]
async fn dotted_spec_column_reads_through_its_relation() -> TestResult {
    let (db, bakery_table, client_table) = setup().await;
    seed_v2_like(&db, &bakery_table, &client_table).await?;

    let (bakery_spec, _) = build_specs(&bakery_table, &client_table);
    let client_yaml = format!(
        r#"
name: client
columns:
  id: {{ type: thing, flags: [id] }}
  name: {{ type: string }}
  bakery: {{ type: thing, references: bakery }}
  bakery.name: {{ type: string }}
surreal:
  table: {client_table}
"#
    );
    let client_spec: SurrealVistaSpec = serde_yaml_ng::from_str(&client_yaml)?;
    let resolver = registry_resolver(vec![("bakery".into(), bakery_spec)]);
    let clients = SurrealVistaFactory::new(db)
        .with_resolver(resolver)
        .build_from_spec(client_spec)?;

    let column = clients.column("bakery.name").expect("dotted column");
    assert!(column.has_flag(ColumnFlag::Calculated));

    let marty = clients.get_value("marty").await?.expect("marty");
    assert_eq!(
        marty.get("bakery.name"),
        Some(&ciborium::Value::Text("Hill Valley Bakery".to_string()))
    );
    Ok(())
}

/// Quicksearch reads every column when none is flagged, but leaves imported
/// reference columns out: text only `bakery.name` holds matches no row.
#[tokio::test]
async fn search_leaves_imported_reference_columns_out() -> TestResult {
    let (db, bakery_table, client_table) = setup().await;
    seed_v2_like(&db, &bakery_table, &client_table).await?;

    let (bakery_spec, _) = build_specs(&bakery_table, &client_table);
    let client_yaml = format!(
        r#"
name: client
columns:
  id: {{ type: thing, flags: [id] }}
  name: {{ type: string }}
  bakery: {{ type: thing, references: bakery }}
  bakery.name: {{ type: string }}
surreal:
  table: {client_table}
"#
    );
    let resolver = registry_resolver(vec![("bakery".into(), bakery_spec)]);
    let factory = SurrealVistaFactory::new(db).with_resolver(resolver);

    let mut by_import = factory.build_from_spec(serde_yaml_ng::from_str(&client_yaml)?)?;
    by_import.set_search(Some("valley bakery"))?;
    assert!(by_import.list_values().await?.is_empty());

    let mut by_name = factory.build_from_spec(serde_yaml_ng::from_str(&client_yaml)?)?;
    by_name.set_search(Some("marty"))?;
    assert_eq!(by_name.list_values().await?.len(), 1);
    Ok(())
}

/// A dotted column through a relation the spec doesn't declare fails the
/// spec load, not the first fetch.
#[test]
fn dotted_spec_column_through_an_unknown_relation_fails_the_build() {
    let db = SurrealDB::new(surreal_client::SurrealMockBuilder::new().build());
    let spec: SurrealVistaSpec = serde_yaml_ng::from_str(
        r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  nope.name: { type: string }
"#,
    )
    .expect("client yaml");
    assert!(SurrealVistaFactory::new(db).build_from_spec(spec).is_err());
}

#[cfg(feature = "rhai")]
const INVOICE: &str = r#"
name: invoice
columns:
  id: { type: thing, flags: [id] }
  client: { type: thing }
  status: { type: string }
"#;

/// A client with one scripted reference, `due_invoices`, whose build script
/// is `script`.
#[cfg(feature = "rhai")]
fn client_with_script(script: &str) -> SurrealVistaSpec {
    let yaml = format!(
        r#"
name: client
columns:
  id: {{ type: thing, flags: [id] }}
  name: {{ type: string }}
references:
  due_invoices:
    table: invoice
    kind: has_many
    foreign_key: client
    surreal:
      rhai: '{script}'
"#
    );
    serde_yaml_ng::from_str(&yaml).expect("client yaml")
}

#[cfg(feature = "rhai")]
async fn seed_invoices(db: &SurrealDB) -> TestResult {
    for sql in [
        "CREATE client:marty SET name = 'Marty'",
        "CREATE client:doc SET name = 'Doc'",
        "CREATE invoice:i1 SET client = client:marty, status = 'due'",
        "CREATE invoice:i2 SET client = client:marty, status = 'paid'",
        "CREATE invoice:i3 SET client = client:doc, status = 'due'",
    ] {
        db.execute(&surreal_expr!(sql)).await?;
    }
    Ok(())
}

/// The book's reference build script: it lands in the reference's
/// `build_script` and, at traversal time, builds the target from the parent
/// `row` in place of the key match.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn scripted_reference_builds_its_target_from_the_parent_row() -> TestResult {
    let (db, _, _) = setup().await;
    seed_invoices(&db).await?;
    let script = r#"table("invoice").where("client", row.id).where("status", "due")"#;
    let client_spec = client_with_script(script);
    let resolver = registry_resolver(vec![
        ("client".into(), client_spec.clone()),
        ("invoice".into(), serde_yaml_ng::from_str(INVOICE)?),
    ]);
    let clients = SurrealVistaFactory::new(db)
        .with_resolver(resolver)
        .build_from_spec(client_spec)?;

    let reference = clients.reference("due_invoices").expect("reference");
    assert_eq!(reference.build_script.as_deref(), Some(script));

    let marty = clients.get_value("marty").await?.expect("marty");
    let due = clients.get_ref("due_invoices", Some(&marty))?;
    let ids: Vec<String> = due.list_values().await?.into_keys().collect();
    assert_eq!(ids, ["invoice:i1"]);
    Ok(())
}

/// Narrows `client` to Marty and `invoice` to the due ones; other keys pass.
#[cfg(feature = "rhai")]
fn marty_and_due() -> vantage_table::table::TableInjector<SurrealDB> {
    use vantage_surrealdb::types::AnySurrealType;
    use vantage_table::traits::table_source::TableSource;

    Arc::new(|cx, table| {
        let (column, value) = match cx.key {
            "client" => ("name", "Marty"),
            "invoice" => ("status", "due"),
            _ => return Ok(()),
        };
        let condition = table
            .data_source()
            .eq_value_condition(column, AnySurrealType::from(value))?;
        table.add_condition(condition);
        Ok(())
    })
}

#[cfg(feature = "rhai")]
async fn sorted_keys(vista: &vantage_vista::Vista) -> vantage_core::Result<Vec<String>> {
    let mut keys: Vec<String> = vista.list_values().await?.into_keys().collect();
    keys.sort();
    Ok(keys)
}

/// The injector reaches the root, a traversed reference, a scripted
/// reference's `table()`, a `base:` table and a `table()` inside `modify:`.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn injector_condition_applies_to_root_reference_base_and_scripts() -> TestResult {
    let (db, _, _) = setup().await;
    seed_invoices(&db).await?;
    let client_spec: SurrealVistaSpec = serde_yaml_ng::from_str(
        r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
references:
  invoices:
    table: invoice
    kind: has_many
    foreign_key: client
  scripted:
    table: invoice
    kind: has_many
    foreign_key: client
    surreal:
      rhai: 'table("invoice").where("client", row.id)'
"#,
    )?;
    let base_spec: SurrealVistaSpec = serde_yaml_ng::from_str(
        r#"
name: invoice_view
columns: {}
surreal:
  base: invoice
  inherit:
    columns: [id, client, status]
"#,
    )?;
    let modify_spec: SurrealVistaSpec = serde_yaml_ng::from_str(
        r#"
name: client_modified
columns:
  id: { type: thing, flags: [id] }
surreal:
  table: client
  modify: 'table("invoice")'
"#,
    )?;
    let resolver = registry_resolver(vec![
        ("client".into(), client_spec.clone()),
        ("invoice".into(), serde_yaml_ng::from_str(INVOICE)?),
    ]);
    let factory = SurrealVistaFactory::new(db)
        .with_resolver(resolver)
        .with_injector(marty_and_due());
    let due = ["invoice:i1", "invoice:i3"];

    let clients = factory.build_from_spec(client_spec)?;
    assert_eq!(sorted_keys(&clients).await?, ["client:marty"]);
    assert_eq!(sorted_keys(&clients.get_ref("invoices", None)?).await?, due);
    let marty = clients.get_value("marty").await?.expect("marty");
    let scripted = clients.get_ref("scripted", Some(&marty))?;
    assert_eq!(sorted_keys(&scripted).await?, ["invoice:i1"]);

    assert_eq!(
        sorted_keys(&factory.build_from_spec(base_spec)?).await?,
        due
    );
    assert_eq!(
        sorted_keys(&factory.build_from_spec(modify_spec)?).await?,
        due
    );
    Ok(())
}

#[tokio::test]
async fn capabilities_report_writes_reach_hooks() -> TestResult {
    use vantage_vista::Capability;

    let (db, bakery_table, client_table) = setup().await;
    let (bakery_spec, _) = build_specs(&bakery_table, &client_table);
    let vista = SurrealVistaFactory::new(db).build_from_spec(bakery_spec)?;
    assert!(vista.capabilities().has(Capability::WritesReachHooks));
    Ok(())
}

/// `with_condition` narrows a Vista in hand: on a handle from `table(name)`
/// it throws an error naming the verb.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn with_condition_on_a_table_handle_is_refused_naming_the_verb() -> TestResult {
    let (db, _, _) = setup().await;
    seed_invoices(&db).await?;
    let client_spec =
        client_with_script(r#"table("invoice").with_condition(surreal::ident("status") == "due")"#);
    let resolver = registry_resolver(vec![
        ("client".into(), client_spec.clone()),
        ("invoice".into(), serde_yaml_ng::from_str(INVOICE)?),
    ]);
    let clients = SurrealVistaFactory::new(db)
        .with_resolver(resolver)
        .build_from_spec(client_spec)?;

    let marty = clients.get_value("marty").await?.expect("marty");
    let err = clients
        .get_ref("due_invoices", Some(&marty))
        .map(drop)
        .expect_err("with_condition on table(...) must fail");
    assert!(err.to_string().contains("with_condition"), "{err}");
    Ok(())
}
