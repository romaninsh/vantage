use ciborium::Value as CborValue;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, ColumnFlag, JoinKey, Reference, Vista, VistaMetadata};

use super::{i, rec, t};

struct Shop {
    client: Vista,
    inventory: Vista,
    orders: Vista,
}

fn id_metadata(extra: &[&str]) -> VistaMetadata {
    extra.iter().fold(
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
            .with_id_column("id"),
        |meta, name| meta.with_column(Column::new(*name, "String")),
    )
}

fn shop() -> Shop {
    let inventory = MockShell::new().with_metadata(id_metadata(&["count", "shelf"]));
    let orders = MockShell::new().with_metadata(id_metadata(&["client_id", "total"]));
    let client_meta = id_metadata(&["name", "inventory_id"])
        .with_reference(Reference::has_one(
            "inventory",
            "inventory",
            JoinKey::to_target_id("inventory_id"),
        ))
        .with_reference(Reference::has_many(
            "orders",
            "order",
            JoinKey::new("id", "client_id"),
        ));
    let client = MockShell::new()
        .with_metadata(client_meta)
        .with_ref_target("inventory", inventory.clone())
        .with_ref_target("orders", orders.clone());
    Shop {
        client: Vista::new("client", client),
        inventory: Vista::new("inventory", inventory),
        orders: Vista::new("order", orders),
    }
}

#[tokio::test]
async fn a_flat_record_inserts_as_one_row() {
    let shop = shop();
    shop.client
        .insert_value("c1".to_string(), &rec(&[("name", t("Ann"))]))
        .await
        .unwrap();
    assert_eq!(shop.client.list_values().await.unwrap().len(), 1);
    assert_eq!(shop.inventory.list_values().await.unwrap().len(), 0);
    assert_eq!(shop.orders.list_values().await.unwrap().len(), 0);
}

#[tokio::test]
async fn a_has_one_child_is_inserted_first_and_its_id_stamped_on_the_parent() {
    let shop = shop();
    let child = CborValue::Map(vec![(t("count"), t("5"))]);
    let id = shop
        .client
        .insert_return_id_value(&rec(&[("name", t("Ann")), ("inventory", child)]))
        .await
        .unwrap();

    let inventory = shop.inventory.list_values().await.unwrap();
    assert_eq!(inventory.len(), 1);
    let (child_id, child_row) = inventory.iter().next().unwrap();
    assert_eq!(child_row.get("count"), Some(&t("5")));

    let parent = shop.client.get_value(&id).await.unwrap().unwrap();
    assert_eq!(parent.get("name"), Some(&t("Ann")));
    assert_eq!(parent.get("inventory_id"), Some(&t(child_id)));
    assert!(
        parent.get("inventory").is_none(),
        "child data is not a column"
    );
}

#[tokio::test]
async fn dotted_keys_of_one_relation_group_into_one_child() {
    let shop = shop();
    let id = shop
        .client
        .insert_return_id_value(&rec(&[
            ("name", t("Ann")),
            ("inventory.count", t("5")),
            ("inventory.shelf", t("a")),
        ]))
        .await
        .unwrap();
    let inventory = shop.inventory.list_values().await.unwrap();
    assert_eq!(inventory.len(), 1);
    let row = inventory.values().next().unwrap();
    assert_eq!(row.get("count"), Some(&t("5")));
    assert_eq!(row.get("shelf"), Some(&t("a")));
    assert!(shop.client.get_value(&id).await.unwrap().is_some());
}

#[tokio::test]
async fn has_many_children_are_inserted_after_the_parent_with_its_id() {
    let shop = shop();
    let orders = CborValue::Array(vec![
        CborValue::Map(vec![(t("total"), t("10"))]),
        CborValue::Map(vec![(t("total"), t("20"))]),
    ]);
    shop.client
        .insert_value(
            "c1".to_string(),
            &rec(&[("name", t("Ann")), ("orders", orders)]),
        )
        .await
        .unwrap();
    let rows = shop.orders.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows.values() {
        assert_eq!(row.get("client_id"), Some(&t("c1")));
    }
    let mut totals: Vec<_> = rows.values().map(|r| r.get("total").cloned()).collect();
    totals.sort_by_key(|v| format!("{v:?}"));
    assert_eq!(totals, [Some(t("10")), Some(t("20"))]);
    let parent = shop.client.get_value("c1").await.unwrap().unwrap();
    assert!(parent.get("orders").is_none());
}

#[tokio::test]
async fn has_many_children_follow_a_backend_made_parent_id() {
    let shop = shop();
    let orders = CborValue::Array(vec![CborValue::Map(vec![(t("total"), t("1"))])]);
    let id = shop
        .client
        .insert_return_id_value(&rec(&[("name", t("Ann")), ("orders", orders)]))
        .await
        .unwrap();
    let rows = shop.orders.list_values().await.unwrap();
    assert_eq!(
        rows.values().next().unwrap().get("client_id"),
        Some(&t(&id))
    );
}

#[tokio::test]
async fn a_bare_scalar_links_an_existing_record_through_the_foreign_key() {
    let shop = shop();
    shop.inventory
        .insert_value("inv9".to_string(), &rec(&[("count", t("3"))]))
        .await
        .unwrap();
    let id = shop
        .client
        .insert_return_id_value(&rec(&[("name", t("Ann")), ("inventory", t("inv9"))]))
        .await
        .unwrap();
    let parent = shop.client.get_value(&id).await.unwrap().unwrap();
    assert_eq!(parent.get("inventory_id"), Some(&t("inv9")));
    assert_eq!(shop.inventory.list_values().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_link_and_child_data_for_one_relation_is_an_error_before_any_write() {
    let shop = shop();
    let err = shop
        .client
        .insert_return_id_value(&rec(&[
            ("name", t("Ann")),
            ("inventory", t("inv9")),
            ("inventory.count", t("1")),
        ]))
        .await
        .unwrap_err();
    assert!(format!("{err:?} {err}").contains("inventory"), "{err:?}");
    assert_eq!(shop.client.list_values().await.unwrap().len(), 0);
    assert_eq!(shop.inventory.list_values().await.unwrap().len(), 0);
}

#[tokio::test]
async fn a_child_of_the_wrong_shape_for_its_relation_is_an_error() {
    let shop = shop();
    let scalar_for_many = shop
        .client
        .insert_return_id_value(&rec(&[("name", t("Ann")), ("orders", t("o1"))]))
        .await;
    assert!(scalar_for_many.is_err());

    let map_for_many = shop
        .client
        .insert_return_id_value(&rec(&[
            ("name", t("Ann")),
            ("orders", CborValue::Map(vec![(t("total"), t("1"))])),
        ]))
        .await;
    assert!(map_for_many.is_err());

    let list_for_one = shop
        .client
        .insert_return_id_value(&rec(&[
            ("name", t("Ann")),
            ("inventory", CborValue::Array(vec![CborValue::Map(vec![])])),
        ]))
        .await;
    assert!(list_for_one.is_err());
    assert_eq!(shop.client.list_values().await.unwrap().len(), 0);
}

#[tokio::test]
async fn a_contained_relation_is_a_plain_field_of_the_row() {
    let lines = VistaMetadata::new().with_column(Column::new("sku", "String"));
    let meta = id_metadata(&[]);
    let shell = MockShell::new()
        .with_metadata(meta)
        .with_contained(Reference::contains_many("lines", "lines"), lines);
    let vista = Vista::new("order", shell);
    let embedded = CborValue::Array(vec![CborValue::Map(vec![(t("sku"), t("a1"))])]);
    vista
        .insert_value("o1".to_string(), &rec(&[("lines", embedded.clone())]))
        .await
        .unwrap();
    let row = vista.get_value("o1").await.unwrap().unwrap();
    assert_eq!(row.get("lines"), Some(&embedded));
    let _ = i(0);
}
