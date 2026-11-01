//! Writes over a connection that signs in as a VIEWER, which SurrealDB
//! refuses with an empty result instead of an error; and writes a vista
//! refuses before they reach the database.

use std::sync::{Arc, Mutex};

#[cfg(feature = "rhai")]
use ciborium::Value as CborValue;
#[cfg(feature = "rhai")]
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_expressions::Expression;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_surrealdb::operation::SurrealOperation;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;
#[cfg(feature = "rhai")]
use vantage_surrealdb::vista::SurrealVistaSpec;
use vantage_table::prelude::Hook;
use vantage_table::table::{AfterCx, Table, after_hook};
use vantage_types::{EmptyEntity, Record};
#[cfg(feature = "rhai")]
use vantage_vista::{Capability, SpecResolver, VistaFactory};

async fn connect(dsn: &str) -> SurrealDB {
    let client = surreal_client::SurrealConnection::dsn(dsn)
        .expect("Invalid DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    SurrealDB::new(client)
}

fn item(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
    Table::new("item", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("parent")
}

fn text(s: &str) -> AnySurrealType {
    AnySurrealType::from(s.to_string())
}

fn rec(pairs: &[(&str, &str)]) -> Record<AnySurrealType> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), text(v)))
        .collect()
}

/// A fresh database, as root.
async fn fresh_root() -> (SurrealDB, String) {
    let name = format!("refused_writes_{}", uuid::Uuid::new_v4().simple());
    let root = connect(&format!("cbor://root:root@localhost:8000/bakery/{name}")).await;
    (root, name)
}

/// A fresh database holding `item:in1`, as root and as a VIEWER of it.
async fn fixture() -> (Table<SurrealDB, EmptyEntity>, Table<SurrealDB, EmptyEntity>) {
    let (root, name) = fresh_root().await;
    root.execute(&Expression::new(
        "DEFINE USER viewer ON DATABASE PASSWORD 'viewer' ROLES VIEWER",
        vec![],
    ))
    .await
    .unwrap();
    let all = item(root);
    all.insert_value(
        Thing::new("item", "in1"),
        &rec(&[("name", "a"), ("parent", "p1")]),
    )
    .await
    .unwrap();
    let viewer = connect(&format!(
        "cbor://viewer:viewer@localhost:8000/bakery/{name}?auth=database"
    ))
    .await;
    (all, item(viewer))
}

fn assert_no_effect(err: vantage_core::VantageError, expected: &str) {
    assert!(err.message().contains(expected), "unexpected error: {err}");
}

#[tokio::test]
async fn refused_delete_errors_and_skips_after_hook() {
    let (all, viewer) = fixture().await;
    let ran = Arc::new(Mutex::new(0));
    let count = ran.clone();
    let hooked = viewer.with_hook(Hook::AfterDelete(after_hook(
        move |_cx: AfterCx<'_, SurrealDB>| {
            *count.lock().unwrap() += 1;
            Box::pin(async { Ok(()) })
        },
    )));
    let err = hooked.delete(Thing::new("item", "in1")).await.unwrap_err();
    assert_no_effect(
        err,
        "delete of `item:in1` had no effect: the connection may not delete it",
    );
    assert_eq!(*ran.lock().unwrap(), 0);
    assert!(
        all.get_value(Thing::new("item", "in1"))
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn delete_of_missing_row_stays_a_no_op() {
    let (all, viewer) = fixture().await;
    viewer.delete(Thing::new("item", "nope")).await.unwrap();
    all.delete(Thing::new("item", "nope")).await.unwrap();
    all.delete(Thing::new("item", "in1")).await.unwrap();
    assert!(
        all.get_value(Thing::new("item", "in1"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn refused_delete_all_errors() {
    let (all, viewer) = fixture().await;
    let set = viewer
        .clone()
        .with_condition(SurrealOperation::eq(&viewer["parent"], text("p1")));
    let err = set.delete_all().await.unwrap_err();
    assert_no_effect(err, "left rows in place");
    assert!(
        all.get_value(Thing::new("item", "in1"))
            .await
            .unwrap()
            .is_some()
    );

    // An empty set has nothing to refuse.
    let empty = viewer
        .clone()
        .with_condition(SurrealOperation::eq(&viewer["parent"], text("p9")));
    empty.delete_all().await.unwrap();
}

#[tokio::test]
async fn refused_insert_errors() {
    let (_all, viewer) = fixture().await;
    let err = viewer
        .insert_value(Thing::new("item", "n2"), &rec(&[("name", "q")]))
        .await
        .unwrap_err();
    assert_no_effect(
        err,
        "insert of `item:n2` had no effect: the connection may not create it",
    );
    let err = viewer
        .insert_return_id_value(&rec(&[("name", "q")]))
        .await
        .unwrap_err();
    assert_no_effect(err, "insert into `item` had no effect");
}

#[tokio::test]
async fn refused_patch_and_replace_error() {
    let (all, viewer) = fixture().await;
    let id = Thing::new("item", "in1");
    let err = viewer
        .patch_value(id.clone(), &rec(&[("name", "z")]))
        .await
        .unwrap_err();
    assert_no_effect(err, "patch of `item:in1` had no effect");
    let err = viewer
        .replace_value(id.clone(), &rec(&[("name", "z"), ("parent", "p1")]))
        .await
        .unwrap_err();
    assert_no_effect(err, "replace of `item:in1` had no effect");
    assert_eq!(all.get_value(id).await.unwrap().unwrap()["name"], text("a"));

    // A patch of a row that isn't there is still not-found.
    let err = viewer
        .patch_value(Thing::new("item", "nope"), &rec(&[("name", "z")]))
        .await
        .unwrap_err();
    assert_eq!(err.kind(), vantage_core::ErrorKind::NotFound);
}

#[cfg(feature = "rhai")]
fn spec(yaml: &str) -> SurrealVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("spec yaml")
}

#[cfg(feature = "rhai")]
fn cbor_record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

/// `brand` refers to `brand_override_view`, a query over `brand_override`.
/// The target is reached through `from_table`, which must see its query
/// source and make it read-only, as `build_from_spec` does (VAN-461).
#[cfg(feature = "rhai")]
#[tokio::test]
async fn a_query_table_reached_by_reference_refuses_writes() {
    let (db, _) = fresh_root().await;
    for sql in [
        "CREATE brand:b1 SET name = 'B1'",
        "CREATE brand_override:o1 SET brand = brand:b1, rollout = 0",
    ] {
        db.execute(&Expression::new(sql, vec![])).await.unwrap();
    }
    let brand = spec(
        r#"
name: brand
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
references:
  override:
    table: brand_override_view
    kind: has_many
    foreign_key: brand
"#,
    );
    let view = spec(
        r#"
name: brand_override_view
columns:
  id: { type: thing, flags: [id] }
  brand: { type: thing }
  rollout: { type: int }
surreal:
  rhai: |
    surreal::select().from("brand_override")
"#,
    );
    let stored = spec(
        r#"
name: brand_override
columns:
  id: { type: thing, flags: [id] }
  rollout: { type: int }
"#,
    );
    let map: IndexMap<String, SurrealVistaSpec> = [("brand_override_view".to_string(), view)]
        .into_iter()
        .collect();
    let resolver: Arc<dyn SpecResolver<SurrealVistaSpec>> =
        Arc::new(move |name: &str| map.get(name).cloned());
    let factory = db.vista_factory().with_resolver(resolver);

    let a = factory.build_from_spec(brand).unwrap();
    let row = a.get_value("brand:b1").await.unwrap().expect("brand:b1");
    let target = a.get_ref("override", Some(&row)).unwrap();
    assert!(!target.capabilities().has(Capability::Update));
    assert!(
        target
            .patch_value(
                "brand_override:o1",
                &cbor_record(&[("rollout", CborValue::Integer(1.into()))])
            )
            .await
            .is_err()
    );

    let plain = factory.build_from_spec(stored).unwrap();
    let after = plain
        .get_value("brand_override:o1")
        .await
        .unwrap()
        .expect("brand_override:o1");
    assert_eq!(after["rollout"], CborValue::Integer(0.into()));
}
