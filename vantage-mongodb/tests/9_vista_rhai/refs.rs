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

/// Narrows `client` to Marty and `order` to the open ones; other keys pass.
fn marty_and_open() -> vantage_table::table::TableInjector<vantage_mongodb::MongoDB> {
    use vantage_mongodb::AnyMongoType;
    use vantage_table::traits::table_source::TableSource;

    std::sync::Arc::new(|cx, table| {
        let (column, value) = match cx.key {
            "client" => ("name", "Marty"),
            "order" => ("status", "open"),
            _ => return Ok(()),
        };
        let condition = table
            .data_source()
            .eq_value_condition(column, AnyMongoType::from(value))?;
        table.add_condition(condition);
        Ok(())
    })
}

async fn sorted_keys(vista: &vantage_vista::Vista) -> vantage_core::Result<Vec<String>> {
    let mut keys: Vec<String> = vista.list_values().await?.into_keys().collect();
    keys.sort();
    Ok(keys)
}

/// The injector reaches the root, a traversed reference, a scripted
/// reference's `table()` and a `table()` inside `modify:`.
#[tokio::test]
async fn injector_condition_applies_to_root_reference_and_scripts() -> TestResult {
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
    let client_spec = spec(&format!(
        "{}  all_orders:\n    table: order\n    kind: has_many\n    foreign_key: client_id\n",
        client_with_script(r#"table("order").where("client_id", me._id)"#)
    ));
    let modify_spec = spec(
        r#"
name: client_modified
columns:
  _id: { type: object_id, flags: [id] }
mongo:
  collection: clients
  modify: 'table("order")'
"#,
    );
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), spec(ORDER)]))
        .with_injector(marty_and_open());

    let clients = factory.build_from_spec(client_spec)?;
    assert_eq!(sorted_keys(&clients).await?, [c1.to_hex()]);
    let all_orders = clients.get_ref("all_orders", None)?;
    assert_eq!(sorted_keys(&all_orders).await?, ["o1", "o3"]);
    let marty = clients.get_value(c1.to_hex().as_str()).await?.expect("c1");
    let scripted = clients.get_ref("orders", Some(&marty))?;
    assert_eq!(sorted_keys(&scripted).await?, ["o1"]);
    let modified = factory.build_from_spec(modify_spec)?;
    assert_eq!(sorted_keys(&modified).await?, ["o1", "o3"]);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn capabilities_of_a_collection_and_a_pipeline_view() -> TestResult {
    let (db, name) = setup().await;
    let orders = db.vista_factory().build_from_spec(spec(ORDER))?;
    assert!(orders.capabilities().has(Capability::Traverse));
    assert!(orders.capabilities().has(Capability::WritesReachHooks));
    // A pipeline view's rows reach other tables through its references too,
    // but it takes no writes.
    let view = db.vista_factory().build_from_spec(spec(BY_CATEGORY))?;
    assert!(view.capabilities().has(Capability::Traverse));
    assert!(!view.capabilities().has(Capability::Insert));
    assert!(!view.capabilities().has(Capability::WritesReachHooks));
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
