//! Contained relations over a JSON host column, built from a typed table
//! and from YAML.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::sqlite::SqliteDB;
use vantage_table::table::{BeforeCx, Hook, Phase, SelectCx, Table, before_hook, select_hook};
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Capability, ReferenceKind, VistaFactory};

use super::TestResult;

/// A `cart` whose `items` column is a JSON array, surfaced as a contains-many
/// relation. SQLite has no native nesting — the collection round-trips as a
/// JSON string in a TEXT column.
fn cart_table(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    Table::<SqliteDB, EmptyEntity>::new("cart", db)
        .with_id_column("id")
        .with_column_of::<String>("items")
        .with_contained_many(
            "items",
            "items",
            |db| {
                Table::<SqliteDB, EmptyEntity>::new("items", db)
                    .with_column_of::<String>("sku")
                    .with_column_of::<i64>("qty")
            },
            None,
        )
}

fn field(name: &str, value: CborValue) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert(name.to_string(), value);
    r
}

/// A dotted key under a contained host is a write to the host: a rule that
/// makes the host read-only refuses it on patch and drops it on replace. A
/// contained record takes no `id` its table doesn't declare. The contained
/// table's update hooks see the stored record, under the host row's id.
#[tokio::test]
async fn a_dotted_contained_key_answers_to_the_host_rule() -> TestResult {
    use vantage_sql::sqlite::AnySqliteType;

    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE person (id TEXT PRIMARY KEY, address TEXT)")
        .execute(db.pool())
        .await?;
    sqlx::query(r#"INSERT INTO person VALUES ('p1', '{"city":"a"}')"#)
        .execute(db.pool())
        .await?;
    let seen: Arc<Mutex<Vec<(Option<String>, Option<CborValue>, bool)>>> = Arc::default();
    let seen_in = seen.clone();
    let people = Table::<SqliteDB, EmptyEntity>::new("person", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("address")
        .with_contained_one("address", "address", move |db| {
            let seen = seen_in.clone();
            Table::<SqliteDB, EmptyEntity>::new("address", db)
                .with_column_of::<String>("city")
                .with_hook(Hook::BeforeUpdate(
                    Phase::Validate,
                    before_hook(move |cx: BeforeCx<'_, SqliteDB>| {
                        seen.lock().unwrap().push((
                            cx.id.cloned(),
                            cx.stored
                                .and_then(|s| s.get("city"))
                                .map(|v| v.value().clone()),
                            cx.patch,
                        ));
                        Box::pin(async { Ok(()) })
                    }),
                ))
        });
    let one = |key: &str, value: &str| -> Record<AnySqliteType> {
        [(key.to_string(), AnySqliteType::from(value.to_string()))]
            .into_iter()
            .collect()
    };
    let stored = || async {
        let (address,): (String,) = sqlx::query_as("SELECT address FROM person WHERE id = 'p1'")
            .fetch_one(db.pool())
            .await
            .unwrap();
        address
    };

    let err = people
        .patch_value("p1", &one("address.id", "x"))
        .await
        .unwrap_err();
    assert!(err.is_forbidden(), "{err}");
    people.patch_value("p1", &one("address.city", "b")).await?;
    assert_eq!(stored().await, r#"{"city":"b"}"#);
    assert_eq!(
        *seen.lock().unwrap(),
        [(
            Some("p1".to_string()),
            Some(CborValue::Text("a".into())),
            true
        )]
    );

    let mut locked = people.clone();
    locked.deny_column("address", "policy/address-locked");
    let err = locked
        .patch_value("p1", &one("address.city", "c"))
        .await
        .unwrap_err();
    assert!(err.is_forbidden(), "{err}");
    locked
        .replace_value("p1", &one("address.city", "c"))
        .await?;
    assert_eq!(stored().await, r#"{"city":"b"}"#);

    // A column aliased under the host (`town` read as `address.city`) doesn't
    // hide the key from the host's rule: the write reaches both.
    let aliased = locked.clone().with_column(
        vantage_table::column::core::Column::<AnySqliteType>::new("town")
            .with_alias("address.city"),
    );
    let err = aliased
        .patch_value("p1", &one("address.city", "c"))
        .await
        .unwrap_err();
    assert!(err.is_forbidden(), "{err}");
    assert_eq!(stored().await, r#"{"city":"b"}"#);

    // A whole host value replaces the record, so it needs no read: a set
    // that may update but not read still takes it.
    let blind = people.clone().with_hook(Hook::BeforeSelect(select_hook(
        |_cx: SelectCx<'_, SqliteDB>| Box::pin(async { Err(vantage_core::error!("no reads")) }),
    )));
    let whole: Record<AnySqliteType> = [(
        "address".to_string(),
        AnySqliteType::untyped(CborValue::Map(vec![("city".into(), "d".into())])),
    )]
    .into_iter()
    .collect();
    blind.patch_value("p1", &whole).await?;
    assert_eq!(stored().await, r#"{"city":"d"}"#);
    Ok(())
}

/// A `json` field of a contained record is stored as the nested object it
/// holds, not as JSON text inside the host's JSON.
#[tokio::test]
async fn a_json_field_in_a_contained_record_stays_an_object() -> TestResult {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE person (id TEXT PRIMARY KEY, address TEXT)")
        .execute(db.pool())
        .await?;
    let people = Table::<SqliteDB, EmptyEntity>::new("person", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("address")
        .with_contained_one("address", "address", |db| {
            Table::<SqliteDB, EmptyEntity>::new("address", db)
                .with_column_of::<serde_json::Value>("meta")
        });
    let vista = db.vista_factory().from_table(people)?;
    let meta = CborValue::Map(vec![("x".into(), CborValue::Integer(1.into()))]);
    vista
        .insert_value(
            &"p1".to_string(),
            &field("address", CborValue::Map(vec![("meta".into(), meta)])),
        )
        .await?;
    let (stored,): (String,) = sqlx::query_as("SELECT address FROM person WHERE id = 'p1'")
        .fetch_one(db.pool())
        .await?;
    assert_eq!(stored, r#"{"meta":{"x":1}}"#);
    Ok(())
}

#[tokio::test]
async fn contained_json_column_round_trips_on_sqlite() -> TestResult {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE cart (id TEXT PRIMARY KEY, items TEXT NOT NULL)")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(r#"INSERT INTO cart VALUES ('c1', '[{"sku":"a","qty":1},{"sku":"b","qty":2}]')"#)
        .execute(db.pool())
        .await
        .unwrap();

    let vista = db.vista_factory().from_table(cart_table(db.clone()))?;
    assert_eq!(
        vista.references()["items"].kind,
        ReferenceKind::ContainsMany
    );

    // Without a parent row, the bare target: the schema, no records, no writes.
    let bare = vista.get_ref("items", None)?;
    assert!(bare.column("sku").is_some());
    assert!(bare.list_values().await?.is_empty());
    assert!(!bare.capabilities().has(Capability::Insert));

    // The host column reads back as a JSON string; the sub-Vista parses it.
    let cart = vista.get_value("c1").await?.unwrap();
    assert!(matches!(cart.get("items"), Some(CborValue::Text(_))));
    let items = vista.get_ref("items", Some(&cart))?;
    assert_eq!(items.list_values().await?.len(), 2);

    // Add an item — eager writeback serializes to JSON and UPDATEs the column.
    let mut line = Record::new();
    line.insert("sku".to_string(), CborValue::Text("c".into()));
    line.insert("qty".to_string(), CborValue::Integer(3i64.into()));
    let new_id = items.insert_return_id_value(&line).await?;
    assert_eq!(new_id, "2");

    // Fresh read + re-traverse re-parses the persisted JSON → three items.
    let cart2 = vista.get_value("c1").await?.unwrap();
    let items2 = vista.get_ref("items", Some(&cart2))?;
    let rows = items2.list_values().await?;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows["2"].get("sku"), Some(&CborValue::Text("c".into())));

    // Patch one item; confirm it persisted through the JSON column.
    items2
        .patch_value("0", &field("qty", CborValue::Integer(99i64.into())))
        .await?;
    let cart3 = vista.get_value("c1").await?.unwrap();
    let items3 = vista.get_ref("items", Some(&cart3))?;
    assert_eq!(
        items3.get_value("0").await?.unwrap().get("qty"),
        Some(&CborValue::Integer(99i64.into()))
    );
    Ok(())
}

#[tokio::test]
async fn contained_from_yaml_round_trips_on_sqlite() -> TestResult {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE cart_yaml (id TEXT PRIMARY KEY, items TEXT NOT NULL)")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(r#"INSERT INTO cart_yaml VALUES ('c1', '[{"sku":"a","qty":1}]')"#)
        .execute(db.pool())
        .await
        .unwrap();

    let yaml = r#"
name: cart
columns:
  id: { type: string, flags: [id] }
  items: { type: string }
sqlite:
  table: cart_yaml
contained:
  items:
    host_column: items
    kind: contains_many
    columns:
      sku: { type: string }
      qty: { type: int }
"#;
    let vista = db.vista_factory().from_yaml(yaml)?;
    let items_ref = &vista.references()["items"];
    assert_eq!(items_ref.kind, ReferenceKind::ContainsMany);
    assert_eq!(items_ref.host_column.as_deref(), Some("items"));

    let cart = vista.get_value("c1").await?.unwrap();
    let items = vista.get_ref("items", Some(&cart))?;
    assert_eq!(items.list_values().await?.len(), 1);

    let mut line = Record::new();
    line.insert("sku".to_string(), CborValue::Text("b".into()));
    line.insert("qty".to_string(), CborValue::Integer(2i64.into()));
    items.insert_return_id_value(&line).await?;

    let cart2 = vista.get_value("c1").await?.unwrap();
    let items2 = vista.get_ref("items", Some(&cart2))?;
    assert_eq!(items2.list_values().await?.len(), 2);
    assert_eq!(
        items2.get_value("1").await?.unwrap().get("sku"),
        Some(&CborValue::Text("b".into()))
    );

    // A write to the host column goes through the line table: a field it
    // doesn't declare is refused, and the stored JSON stays as it was.
    let line = CborValue::Map(vec![
        ("sku".into(), "c".into()),
        ("secret".into(), "x".into()),
    ]);
    let patch: Record<CborValue> = [("items".to_string(), CborValue::Array(vec![line]))]
        .into_iter()
        .collect();
    let err = vista
        .patch_value("c1".to_string(), &patch)
        .await
        .expect_err("an undeclared line field is refused");
    assert!(err.to_string().contains("secret"), "{err}");
    let (stored,): (String,) = sqlx::query_as("SELECT items FROM cart_yaml WHERE id = 'c1'")
        .fetch_one(db.pool())
        .await?;
    assert_eq!(stored, r#"[{"sku":"a","qty":1},{"sku":"b","qty":2}]"#);

    // A contains_many collection has no single path for a dotted key.
    let dotted: Record<CborValue> = [("items.qty".to_string(), CborValue::Integer(3.into()))]
        .into_iter()
        .collect();
    let err = vista
        .patch_value("c1".to_string(), &dotted)
        .await
        .expect_err("a dotted key into a contains_many is refused");
    assert!(err.is_forbidden(), "{err}");
    Ok(())
}
