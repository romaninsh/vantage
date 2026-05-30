//! Contained relations against a real SurrealDB.
//!
//! Requires a running instance: `cd scripts && ./start.sh && ./ingress.sh`.
//! Self-contained and re-runnable — operates on a throwaway schemaless
//! `contained_test` record it creates and deletes.

use ciborium::Value as CborValue;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::types::AnySurrealType;
use vantage_surrealdb::vista::factory::SurrealVistaFactory;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

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
    let lines = vista.get_ref("lines", &row).unwrap();
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
        .patch_value(&"0".to_string(), &rec(&[("a", int(99))]))
        .await
        .unwrap();
    let after2 = vista.get_value(&id).await.unwrap().unwrap();
    let CborValue::Array(stored2) = after2.get("lines").unwrap() else {
        panic!("lines should be an array");
    };
    assert_eq!(stored2[0], lmap(&[("a", int(99))]));

    vista.delete(&id).await.unwrap();
}
