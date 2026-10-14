//! Scripted references: the script builds the target, and the capabilities a
//! scripted reference advertises.

use bson::doc;
use bson::oid::ObjectId;
use vantage_dataset::prelude::*;
use vantage_vista::{Capability, VistaFactory, contract};

use super::{
    BY_CATEGORY, CLIENT, ORDER, PLAIN_PRODUCT, TestResult, any_client_row, client_with_script,
    resolver, seed_products, setup, spec, teardown, text,
};

const CATEGORY_GROUPS: &str = r#"
name: category_groups
columns:
  _id: { type: string, flags: [id] }
  n: { type: int }
mongo:
  collection: products
  pipeline:
    - $group: { _id: "$category", n: { $sum: 1 } }
references:
  products:
    table: product
    kind: has_many
    foreign_key: category
    mongo:
      rhai: |
        table("product").where("category", me._id)
"#;

#[tokio::test]
async fn build_script_reference_resolves_target() -> TestResult {
    let (db, name) = setup().await;
    let (c1, c2) = (ObjectId::new(), ObjectId::new());
    db.doc_collection("clients")
        .insert_many(vec![
            doc! { "_id": c1, "name": "Marty" },
            doc! { "_id": c2, "name": "Doc" },
        ])
        .await?;
    db.doc_collection("orders")
        .insert_many(vec![
            doc! { "_id": "o1", "client_id": c1, "item": "skateboard", "status": "open" },
            doc! { "_id": "o2", "client_id": c1, "item": "almanac", "status": "closed" },
            doc! { "_id": "o3", "client_id": c2, "item": "flux capacitor", "status": "open" },
        ])
        .await?;

    let (client_spec, order_spec) = (spec(CLIENT), spec(ORDER));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), order_spec]));
    let clients = factory.build_from_spec(client_spec)?;
    let reference = clients.reference("orders").expect("orders ref");
    assert!(reference.build_script.is_some());

    let marty = clients
        .get_value(c1.to_hex().as_str())
        .await?
        .expect("client c1");
    let orders = clients.get_ref("orders", Some(&marty))?;
    assert_eq!(orders.name(), "order");
    let rows = orders.list_values().await?;
    let items: Vec<String> = rows.values().filter_map(|r| text(r, "item")).collect();
    // The script's extra `status` condition proves it ran, not the FK path.
    assert_eq!(items, vec!["skateboard"]);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn build_script_reference_refuses_with_condition_on_table() -> TestResult {
    let (db, name) = setup().await;
    let client =
        client_with_script(r#"table("order").with_condition(#{client_id: mongo::oid(row._id)})"#);
    let (client_spec, order_spec) = (spec(&client), spec(ORDER));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), order_spec]));
    let clients = factory.build_from_spec(client_spec)?;

    let err = clients
        .get_ref("orders", Some(&any_client_row()))
        .map(drop)
        .expect_err("with_condition on table(...) must fail");
    assert!(
        err.to_string().contains(r#"verb: "with_condition""#),
        "{err}"
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn capabilities_advertise_traverse_for_scripted_refs() -> TestResult {
    let (db, name) = setup().await;
    let v = db.vista_factory().build_from_spec(spec(ORDER))?;
    assert!(v.capabilities().has(Capability::Traverse));
    // A pipeline view's rows reach other tables through its references too.
    let view = db.vista_factory().build_from_spec(spec(BY_CATEGORY))?;
    assert!(view.capabilities().has(Capability::Traverse));
    assert!(!view.capabilities().has(Capability::Insert));
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn scripted_ref_from_group_view_row() -> TestResult {
    let (db, name) = setup().await;
    seed_products(&db).await?;

    let (groups_spec, product_spec) = (spec(CATEGORY_GROUPS), spec(PLAIN_PRODUCT));
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![groups_spec.clone(), product_spec]));
    let groups = factory.build_from_spec(groups_spec)?;
    let cake = groups.get_value("cake").await?.expect("cake group");
    let products = groups.get_ref("products", Some(&cake))?;
    assert_eq!(products.name(), "product");
    assert_eq!(products.count().await?, 3);
    // The target is the writable collection, not the view.
    assert!(products.capabilities().has(Capability::Insert));
    contract::check_advertised(&groups).await;

    teardown(&db, &name).await;
    Ok(())
}
