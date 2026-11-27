//! Contained relations against a real SurrealDB.
//!
//! Requires a running instance: `cd scripts && ./start.sh && ./ingress.sh`.
//! Self-contained and re-runnable — operates on a throwaway schemaless
//! `contained_test` record it creates and deletes.

#![cfg(feature = "vista")]

use ciborium::Value as CborValue;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_expressions::ExprDataSource as _;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::types::AnySurrealType;
use vantage_surrealdb::vista::factory::SurrealVistaFactory;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::VistaFactory as _;

/// Both tests write `contained_test` rows. SurrealDB answers concurrent
/// writes to one table with a transaction conflict, so they take turns.
static TABLE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn v2_db() -> SurrealDB {
    let dsn = "cbor://root:root@localhost:8000/bakery/v2";
    let client = surreal_client::SurrealConnection::dsn(dsn)
        .expect("valid DSN")
        .connect()
        .await
        .expect("connect to SurrealDB — is it running? (scripts/start.sh)");
    SurrealDB::new(client)
}

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn lmap(pairs: &[(&str, CborValue)]) -> CborValue {
    CborValue::Map(
        pairs
            .iter()
            .map(|(k, v)| (CborValue::Text((*k).into()), v.clone()))
            .collect(),
    )
}

fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).into(), v.clone()))
        .collect()
}

fn test_table(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
    Table::new("contained_test", db)
        .with_id_column("id")
        .with_column_of::<AnySurrealType>("lines")
        .with_contained_many(
            "lines",
            "lines",
            |db| Table::new("lines", db).with_column_of::<i64>("a"),
            None,
        )
}

#[tokio::test]
async fn contains_many_eager_writeback_on_real_surreal() {
    let _turn = TABLE_LOCK.lock().await;
    let db = v2_db().await;
    let vista = SurrealVistaFactory::new(db.clone())
        .from_table(test_table(db.clone()))
        .unwrap();
    let id = "contained_test:t1".to_string();

    // Clean slate (ignore "not found" on first run).
    let _ = vista.delete(&id).await;

    // Seed a row whose `lines` column embeds two objects.
    vista
        .insert_value(
            &id,
            &rec(&[(
                "lines",
                CborValue::Array(vec![lmap(&[("a", int(1))]), lmap(&[("a", int(2))])]),
            )]),
        )
        .await
        .unwrap();

    // Traverse the embedded array as a sub-Vista.
    let row = vista.get_value(&id).await.unwrap().unwrap();
    let lines = vista.get_ref("lines", Some(&row)).unwrap();
    assert_eq!(lines.list_values().await.unwrap().len(), 2);

    // Insert a third line — eager writeback patches the parent row.
    let new_id = lines
        .insert_return_id_value(&rec(&[("a", int(3))]))
        .await
        .unwrap();
    assert_eq!(new_id, "2");

    let after = vista.get_value(&id).await.unwrap().unwrap();
    let CborValue::Array(stored) = after.get("lines").unwrap() else {
        panic!("lines should be an array");
    };
    assert_eq!(stored.len(), 3);
    assert_eq!(stored[2], lmap(&[("a", int(3))]));

    // Patch the first line; re-read the parent and confirm it landed.
    lines
        .patch_value("0", &rec(&[("a", int(99))]))
        .await
        .unwrap();
    let after2 = vista.get_value(&id).await.unwrap().unwrap();
    let CborValue::Array(stored2) = after2.get("lines").unwrap() else {
        panic!("lines should be an array");
    };
    assert_eq!(stored2[0], lmap(&[("a", int(99))]));

    vista.delete(&id).await.unwrap();
}

/// A `contains_one` host given as JSON text is stored as the object it holds.
/// A whole host value replaces the stored record: a field the new value
/// leaves out is gone, not merged back in.
#[tokio::test]
async fn a_whole_host_patch_replaces_the_contained_record() {
    let _turn = TABLE_LOCK.lock().await;
    let db = v2_db().await;
    let table = Table::<SurrealDB, EmptyEntity>::new("contained_test", db.clone())
        .with_id_column("id")
        .with_column_of::<AnySurrealType>("address")
        .with_contained_one("address", "address", |db| {
            Table::new("address", db)
                .with_column_of::<String>("city")
                .with_column_of::<String>("zip")
        });
    let vista = SurrealVistaFactory::new(db.clone())
        .from_table(table)
        .unwrap();
    let id = "contained_test:t3".to_string();
    let _ = vista.delete(&id).await;
    vista
        .insert_value(
            &id,
            &rec(&[("address", text(r#"{"city":"Rome","zip":"00100"}"#))]),
        )
        .await
        .unwrap();
    let row = vista.get_value(&id).await.unwrap().unwrap();
    assert_eq!(
        row["address"],
        lmap(&[("city", text("Rome")), ("zip", text("00100"))])
    );

    vista
        .patch_value(&id, &rec(&[("address", lmap(&[("city", text("Paris"))]))]))
        .await
        .unwrap();
    let row = vista.get_value(&id).await.unwrap().unwrap();
    assert_eq!(row["address"], lmap(&[("city", text("Paris"))]));

    vista.delete(&id).await.unwrap();
}

/// A `nested_path` column reads one field of its host's object, and a patch
/// through it changes that field and keeps the others.
#[tokio::test]
async fn a_nested_path_patch_keeps_the_hosts_other_fields() {
    let _turn = TABLE_LOCK.lock().await;
    let db = v2_db().await;
    let vista = SurrealVistaFactory::new(db.clone())
        .from_yaml(
            r#"
name: contained_test
columns:
  id: { type: thing, flags: [id] }
  name: { type: json, flags: [hidden] }
  label: { type: string, nested_path: name.en_US }
"#,
        )
        .unwrap();
    let id = "contained_test:t5".to_string();
    db.execute(&surreal_expr!(
        r#"UPSERT contained_test:t5 CONTENT { name: { en_US: "Bug", de_DE: "Fehler" } }"#
    ))
    .await
    .unwrap();
    let row = vista.get_value(&id).await.unwrap().unwrap();
    assert_eq!(row["label"], text("Bug"));

    vista
        .patch_value(&id, &rec(&[("label", text("Design"))]))
        .await
        .unwrap();
    let row = vista.get_value(&id).await.unwrap().unwrap();
    assert_eq!(row["label"], text("Design"));
    let CborValue::Map(name) = &row["name"] else {
        panic!("name should be an object: {row:?}");
    };
    assert!(name.contains(&(text("de_DE"), text("Fehler"))), "{row:?}");
    assert!(name.contains(&(text("en_US"), text("Design"))), "{row:?}");

    vista.delete(&id).await.unwrap();
}

/// An `expr:` falls back from one translation to another: `surreal::json`
/// reads a key (`@` too), and `table.column` a nested column.
#[tokio::test]
async fn name_local_falls_back() {
    let _turn = TABLE_LOCK.lock().await;
    let db = v2_db().await;
    let vista = SurrealVistaFactory::new(db.clone())
        .from_yaml(
            r#"
name: contained_test
columns:
  id: { type: thing, flags: [id] }
  name: { type: json, flags: [hidden] }
  name_en: { type: string, nested_path: name.en_US }
  name_local: { type: string, expr: 'surreal::coalesce(surreal::json(table.column("name"), "de_DE"), table.column("name_en"))' }
  name_sr: { type: string, expr: 'surreal::json(surreal::ident("name"), "sr@latin")' }
"#,
        )
        .unwrap();
    db.execute(&surreal_expr!(
        r#"UPSERT contained_test:t7 CONTENT { name: { en_US: "Defect", de_DE: "Fehler", "sr@latin": "Greška" } }"#
    ))
    .await
    .unwrap();
    db.execute(&surreal_expr!(
        r#"UPSERT contained_test:t8 CONTENT { name: { en_US: "Bug" } }"#
    ))
    .await
    .unwrap();

    let de = vista.get_value("contained_test:t7").await.unwrap().unwrap();
    assert_eq!(de["name_local"], text("Fehler"), "{de:?}");
    assert_eq!(de["name_sr"], text("Greška"), "{de:?}");
    let en = vista.get_value("contained_test:t8").await.unwrap().unwrap();
    assert_eq!(en["name_local"], text("Bug"), "{en:?}");

    vista.delete("contained_test:t7").await.unwrap();
    vista.delete("contained_test:t8").await.unwrap();
}

/// A `nested_path` column saved as null removes its field from the host;
/// the other fields stay.
#[tokio::test]
async fn null_removes_key() {
    let _turn = TABLE_LOCK.lock().await;
    let db = v2_db().await;
    let vista = SurrealVistaFactory::new(db.clone())
        .from_yaml(
            r#"
name: contained_test
columns:
  id: { type: thing, flags: [id] }
  name: { type: json, flags: [hidden] }
  name_en: { type: string, nested_path: name.en_US }
  name_de: { type: string, nested_path: name.de_DE }
"#,
        )
        .unwrap();
    let id = "contained_test:t6".to_string();
    db.execute(&surreal_expr!(
        r#"UPSERT contained_test:t6 CONTENT { name: { en_US: "Bug", de_DE: "Fehler" } }"#
    ))
    .await
    .unwrap();

    vista
        .patch_value(&id, &rec(&[("name_de", CborValue::Null)]))
        .await
        .unwrap();
    let row = vista.get_value(&id).await.unwrap().unwrap();
    assert_eq!(row["name"], lmap(&[("en_US", text("Bug"))]), "{row:?}");
    assert_eq!(row["name_en"], text("Bug"));

    // What if the host is written whole with a key no nested column reads:
    // the host is a free-form object, so the key is kept.
    vista
        .patch_value(
            &id,
            &rec(&[("name", lmap(&[("en_US", text("X")), ("fr_FR", text("Y"))]))]),
        )
        .await
        .unwrap();
    let row = vista.get_value(&id).await.unwrap().unwrap();
    assert_eq!(
        row["name"],
        lmap(&[("en_US", text("X")), ("fr_FR", text("Y"))]),
        "{row:?}"
    );

    vista.delete(&id).await.unwrap();
}

/// A `nested_path` column reads and writes one place: a column or host
/// stored under another field is refused when the table loads, and so is a
/// nested column on a `base:` vista.
#[tokio::test]
async fn a_nested_path_with_a_second_location_is_refused() {
    use std::sync::Arc;
    use vantage_surrealdb::vista::SurrealVistaSpec;
    use vantage_vista::SpecResolver;

    let db = v2_db().await;
    let tag = |label: &str, name: &str| {
        format!(
            "name: contained_test\ncolumns:\n  id: {{ type: thing, flags: [id] }}\n  name: {name}\n  label: {label}\n"
        )
    };
    let nested = "{ type: string, nested_path: name.en_US }";
    let json = "{ type: json }";
    for (yaml, refusal) in [
        (
            tag(
                "{ type: string, nested_path: name.en_US, surreal: { field: name.fr_FR } }",
                json,
            ),
            "a nested_path column is stored at its path",
        ),
        (
            tag(nested, "{ type: json, surreal: { field: translations } }"),
            "a nested_path host is read at its own name",
        ),
    ] {
        let err = SurrealVistaFactory::new(db.clone())
            .from_yaml(&yaml)
            .unwrap_err()
            .to_string();
        assert!(err.contains(refusal), "{err}");
    }

    let base: SurrealVistaSpec = serde_yaml_ng::from_str(&tag(nested, json)).unwrap();
    let resolver: Arc<dyn SpecResolver<SurrealVistaSpec>> =
        Arc::new(move |name: &str| (name == "contained_test").then(|| base.clone()));
    let derived = format!(
        "name: tag_view\ncolumns:\n  title: {nested}\nsurreal:\n  base: contained_test\n  inherit:\n    columns: [id]\n"
    );
    let err = SurrealVistaFactory::new(db)
        .with_resolver(resolver)
        .build_from_spec(serde_yaml_ng::from_str(&derived).unwrap())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("nested_path isn't supported on a `base:` vista"),
        "{err}"
    );
}

/// Without a parent row a contained relation is its bare target: the
/// embedded table with the columns its builder declares.
#[tokio::test]
async fn contained_relation_without_a_row_is_its_bare_target() {
    let db = v2_db().await;
    let vista = SurrealVistaFactory::new(db.clone())
        .from_table(test_table(db))
        .unwrap();
    let lines = vista.get_ref("lines", None).unwrap();
    assert!(lines.column("a").is_some());
}

/// A simple `product`-shaped vista for the traverse-out test.
fn product_table(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
    Table::new("product", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
}

/// `contained_test` whose embedded lines each carry a `product` record link and
/// traverse out to the real `product` table.
fn linked_table(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
    Table::new("contained_test", db)
        .with_id_column("id")
        .with_column_of::<AnySurrealType>("lines")
        .with_contained_many(
            "lines",
            "lines",
            |db| {
                Table::new("lines", db)
                    .with_column_of::<vantage_surrealdb::thing::Thing>("product")
                    .with_one("product", "product", product_table)
            },
            None,
        )
}

#[tokio::test]
async fn contained_record_traverses_out_to_product() {
    let _turn = TABLE_LOCK.lock().await;
    let db = v2_db().await;
    let vista = SurrealVistaFactory::new(db.clone())
        .from_table(linked_table(db.clone()))
        .unwrap();
    let id = "contained_test:t2".to_string();
    let _ = vista.delete(&id).await;

    // Seed a line whose `product` links to the seeded product:flux_cupcake.
    let product_link = vantage_surrealdb::types::AnySurrealType::new(
        vantage_surrealdb::thing::Thing::new("product", "flux_cupcake"),
    )
    .into_value();
    vista
        .insert_value(
            &id,
            &rec(&[(
                "lines",
                CborValue::Array(vec![lmap(&[("product", product_link)])]),
            )]),
        )
        .await
        .unwrap();

    // line -> product traversal resolves to the real product row.
    let row = vista.get_value(&id).await.unwrap().unwrap();
    let lines = vista.get_ref("lines", Some(&row)).unwrap();
    let line0 = lines.get_value("0").await.unwrap().unwrap();
    let product = lines.get_ref("product", Some(&line0)).unwrap();
    let (_pid, prod) = product.get_some_value().await.unwrap().unwrap();
    assert_eq!(prod.get("name"), Some(&text("Flux Capacitor Cupcake")));

    vista.delete(&id).await.unwrap();
}
