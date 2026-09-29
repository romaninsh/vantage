use vantage_memory::MemoryDB;
use vantage_memory::prelude::*;
use vantage_table::prelude::*;
use vantage_types::{EmptyEntity, Record};

fn products(db: &MemoryDB) -> Table<MemoryDB, EmptyEntity> {
    Table::new("product", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<String>("category")
}

async fn seeded() -> MemoryDB {
    let db = MemoryDB::new();
    let t = products(&db);
    for (id, name, price, cat) in [
        ("p1", "Tea", 3, "drink"),
        ("p2", "Cake", 5, "food"),
        ("p3", "Coffee", 4, "drink"),
    ] {
        let r: Record<AnyMemoryType> = [
            ("name".to_string(), AnyMemoryType::from(name)),
            ("price".to_string(), AnyMemoryType::from(price as i64)),
            ("category".to_string(), AnyMemoryType::from(cat)),
        ]
        .into_iter()
        .collect();
        t.data_source()
            .insert_table_value(&t, &id.to_string(), &r)
            .await
            .unwrap();
    }
    db
}

#[tokio::test]
async fn conditions_filter_list_and_count() {
    let db = seeded().await;
    let mut t = products(&db);
    t.add_condition(t["price"].gt(3));
    let rows = t.data_source().list_table_values(&t).await.unwrap();
    assert_eq!(rows.keys().cloned().collect::<Vec<_>>(), ["p2", "p3"]);
    assert_eq!(t.data_source().get_table_count(&t).await.unwrap(), 2);
}

#[tokio::test]
async fn order_and_pagination() {
    let db = seeded().await;
    let mut t = products(&db);
    t.add_order(t["price"].descending());
    t.set_pagination(Some(Pagination::window(1, 1)));
    let rows = t.data_source().list_table_values(&t).await.unwrap();
    assert_eq!(rows.keys().cloned().collect::<Vec<_>>(), ["p3"]);
}

#[tokio::test]
async fn aggregates_respect_conditions() {
    let db = seeded().await;
    let mut t = products(&db);
    t.add_condition(t["category"].eq("drink"));
    let price = t["price"].clone();
    let sum = t.data_source().get_table_sum(&t, &price).await.unwrap();
    let max = t.data_source().get_table_max(&t, &price).await.unwrap();
    let min = t.data_source().get_table_min(&t, &price).await.unwrap();
    assert_eq!(i64::try_from(sum).unwrap(), 7);
    assert_eq!(i64::try_from(max).unwrap(), 4);
    assert_eq!(i64::try_from(min).unwrap(), 3);
}

#[tokio::test]
async fn crud_round_trip() {
    let db = seeded().await;
    let t = products(&db);
    let ds = t.data_source();
    let id = ds
        .insert_table_return_id_value(&t, &Record::new())
        .await
        .unwrap();
    assert_eq!(id, "1");
    let patch: Record<AnyMemoryType> = [("name".to_string(), AnyMemoryType::from("Scone"))]
        .into_iter()
        .collect();
    ds.patch_table_value(&t, &id, &patch).await.unwrap();
    let got = ds.get_table_value(&t, &id).await.unwrap().unwrap();
    assert_eq!(String::try_from(got["name"].clone()).unwrap(), "Scone");
    assert!(
        ds.insert_table_value(&t, &"p1".to_string(), &Record::new())
            .await
            .is_err()
    );
    ds.delete_table_value(&t, &id).await.unwrap();
    assert!(ds.delete_table_value(&t, &id).await.is_err());
    assert!(ds.patch_table_value(&t, &id, &patch).await.is_err());
}

#[tokio::test]
async fn delete_all_honours_conditions() {
    let db = seeded().await;
    let mut t = products(&db);
    t.add_condition(t["category"].eq("drink"));
    t.data_source().delete_table_all_values(&t).await.unwrap();
    assert_eq!(db.store().table("product").ids(), vec!["p2"]);
}

#[tokio::test]
async fn search_matches_text_cells() {
    let db = seeded().await;
    let mut t = products(&db);
    let c = t.data_source().search_table_condition(&t, "cof");
    t.add_condition(c);
    let rows = t.data_source().list_table_values(&t).await.unwrap();
    assert_eq!(rows.keys().cloned().collect::<Vec<_>>(), ["p3"]);
}

#[tokio::test]
async fn related_in_condition_narrows_by_source_rows() {
    let db = seeded().await;
    let orders = db.store().table("order");
    orders.upsert(
        "o1",
        [("product".to_string(), ciborium::Value::Text("p2".into()))]
            .into_iter()
            .collect(),
    );
    orders.upsert(
        "o2",
        [("product".to_string(), ciborium::Value::Text("p1".into()))]
            .into_iter()
            .collect(),
    );
    let mut src = products(&db);
    src.add_condition(src["category"].eq("food"));
    let mut o = Table::<MemoryDB, EmptyEntity>::new("order", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("product");
    let c = db.related_in_condition("product", &src, "id");
    o.add_condition(c);
    let rows = o.data_source().list_table_values(&o).await.unwrap();
    assert_eq!(rows.keys().cloned().collect::<Vec<_>>(), ["o1"]);
}

#[tokio::test]
async fn related_in_condition_matches_integer_foreign_keys() {
    let db = seeded().await;
    let clients = db.store().table("client");
    for id in ["1", "2"] {
        clients.upsert(id, Record::new());
    }
    let orders = db.store().table("order");
    for (id, client) in [("o1", 1), ("o2", 2)] {
        orders.upsert(
            id,
            [(
                "client_id".to_string(),
                ciborium::Value::Integer(client.into()),
            )]
            .into_iter()
            .collect(),
        );
    }
    let mut src = Table::<MemoryDB, EmptyEntity>::new("client", db.clone()).with_id_column("id");
    src.add_condition(src["id"].eq("2"));
    let mut o = Table::<MemoryDB, EmptyEntity>::new("order", db.clone())
        .with_id_column("id")
        .with_column_of::<i64>("client_id");
    let c = db.related_in_condition("client_id", &src, "id");
    o.add_condition(c);
    let rows = o.data_source().list_table_values(&o).await.unwrap();
    assert_eq!(rows.keys().cloned().collect::<Vec<_>>(), ["o2"]);
}

#[tokio::test]
async fn id_column_mismatch_with_existing_table_errors() {
    let db = seeded().await;
    let t = Table::<MemoryDB, EmptyEntity>::new("product", db.clone()).with_id_column("sku");
    assert!(t.data_source().list_table_values(&t).await.is_err());
}
