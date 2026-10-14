//! ObjectId foreign keys: traversal reads them, and writes through a
//! traversed set store them as ObjectIds again.

use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_vista::VistaFactory;

use super::{CLIENT, ORDER, TestResult, resolver, setup, spec, teardown, text};

#[tokio::test]
async fn references_traverse_object_id_foreign_keys() -> TestResult {
    let (db, name) = setup().await;
    let (c1, c2) = (ObjectId::new(), ObjectId::new());
    db.doc_collection("clients")
        .insert_many(vec![
            bson::doc! { "_id": c1, "name": "Marty", "address": { "city": "Hill Valley" } },
            bson::doc! { "_id": c2, "name": "Doc", "address": { "city": "Twin Pines" } },
        ])
        .await?;
    db.doc_collection("orders")
        .insert_many(vec![
            bson::doc! { "_id": "o1", "client_id": c1, "item": "skateboard" },
            bson::doc! { "_id": "o2", "client_id": c1, "item": "almanac" },
            bson::doc! { "_id": "o3", "client_id": c2, "item": "flux capacitor" },
        ])
        .await?;

    let (client_spec, order_spec) = (spec(CLIENT), spec(ORDER));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), order_spec.clone()]));

    let clients = factory.build_from_spec(client_spec)?;
    let marty = clients
        .get_value(c1.to_hex().as_str())
        .await?
        .expect("client c1");
    let orders = clients.get_ref("orders", Some(&marty))?;
    let rows = orders.list_values().await?;
    let mut items: Vec<String> = rows.values().filter_map(|r| text(r, "item")).collect();
    items.sort();
    assert_eq!(items, vec!["almanac", "skateboard"]);

    // Inserting through the traversed set writes the foreign key as an ObjectId.
    let mut hoverboard = vantage_types::Record::new();
    hoverboard.insert("item".to_string(), CborValue::Text("hoverboard".into()));
    let new_id = orders.insert_return_id_value(&hoverboard).await?;
    let stored = db
        .doc_collection("orders")
        .find_one(bson::doc! { "_id": ObjectId::parse_str(&new_id)? })
        .await?
        .expect("inserted order");
    assert_eq!(stored.get("client_id"), Some(&bson::Bson::ObjectId(c1)));

    let orders = factory.build_from_spec(order_spec)?;
    let o3 = orders.get_value("o3").await?.expect("order o3");
    let client = orders.get_ref("client", Some(&o3))?;
    let rows = client.list_values().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        text(rows.values().next().unwrap(), "name").as_deref(),
        Some("Doc")
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn writes_through_traversed_set_accept_ids_as_read() -> TestResult {
    let (db, name) = setup().await;
    let c1 = ObjectId::new();
    db.doc_collection("clients")
        .insert_one(bson::doc! { "_id": c1, "name": "Marty" })
        .await?;
    db.doc_collection("orders")
        .insert_one(bson::doc! { "_id": "o1", "client_id": c1, "item": "skateboard" })
        .await?;

    let (client_spec, order_spec) = (spec(CLIENT), spec(ORDER));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), order_spec]));
    let clients = factory.build_from_spec(client_spec)?;
    let marty = clients
        .get_value(c1.to_hex().as_str())
        .await?
        .expect("client");
    let orders = clients.get_ref("orders", Some(&marty))?;

    let stored_fk = || async {
        db.doc_collection("orders")
            .find_one(bson::doc! { "_id": "o1" })
            .await
            .map(|d| d.and_then(|d| d.get("client_id").cloned()))
    };

    // Replace and patch carry client_id exactly as it was read.
    let mut order = orders.get_value("o1").await?.expect("order o1");
    assert_eq!(text(&order, "client_id"), Some(c1.to_hex()));
    order.insert("item".into(), CborValue::Text("longboard".into()));
    orders.replace_value("o1", &order).await?;
    assert_eq!(stored_fk().await?, Some(bson::Bson::ObjectId(c1)));

    let mut partial = vantage_types::Record::new();
    partial.insert("client_id".to_string(), CborValue::Text(c1.to_hex()));
    partial.insert("item".to_string(), CborValue::Text("hoverboard".into()));
    orders.patch_value("o1", &partial).await?;
    assert_eq!(stored_fk().await?, Some(bson::Bson::ObjectId(c1)));

    // Insert with the foreign key prefilled from the parent's hex id.
    let mut fresh = vantage_types::Record::new();
    fresh.insert("client_id".to_string(), CborValue::Text(c1.to_hex()));
    fresh.insert("item".to_string(), CborValue::Text("almanac".into()));
    let id = orders.insert_return_id_value(&fresh).await?;
    let stored = db
        .doc_collection("orders")
        .find_one(bson::doc! { "_id": ObjectId::parse_str(&id)? })
        .await?
        .expect("inserted order");
    assert_eq!(stored.get("client_id"), Some(&bson::Bson::ObjectId(c1)));

    teardown(&db, &name).await;
    Ok(())
}
