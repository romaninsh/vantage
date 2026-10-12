use ciborium::Value as CborValue;
use vantage_types::Record;

use crate::{Column, JoinKey, Reference, Vista, VistaMetadata, mocks::MockShell};

/// Vista over a `client` table: scalar columns plus a has-one `bakery`
/// (FK `bakery_id` on this row) and a has-many `orders` (FK `client_id` on
/// the order row).
fn client_vista() -> Vista {
    client_vista_over(MockShell::new(), MockShell::new())
}

/// [`client_vista`] with the given bakery and order stores as join targets.
fn client_vista_over(bakeries: MockShell, orders: MockShell) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id")
        .with_reference(Reference::has_one(
            "bakery",
            "bakery",
            JoinKey::new("bakery_id", "id"),
        ))
        .with_reference(Reference::has_many(
            "orders",
            "order",
            JoinKey::new("id", "client_id"),
        ));
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_ref_target("bakery", bakeries)
        .with_ref_target("orders", orders);
    Vista::new("client", shell)
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn map(pairs: &[(&str, CborValue)]) -> CborValue {
    CborValue::Map(
        pairs
            .iter()
            .map(|(k, v)| (CborValue::Text((*k).into()), v.clone()))
            .collect(),
    )
}

fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).into(), v.clone()))
        .collect()
}

#[test]
fn flat_record_has_no_relations() {
    let (main, has_one, has_many) = client_vista()
        .classify_insert(&record(&[
            ("name", text("John")),
            ("bakery_id", text("b1")),
        ]))
        .unwrap();
    assert_eq!(main.get("name"), Some(&text("John")));
    assert_eq!(main.get("bakery_id"), Some(&text("b1")));
    assert!(has_one.is_empty());
    assert!(has_many.is_empty());
}

#[test]
fn dotted_keys_group_into_one_has_one_child() {
    let (main, has_one, has_many) = client_vista()
        .classify_insert(&record(&[
            ("name", text("John")),
            ("bakery.name", text("New Bakery")),
            ("bakery.profit_margin", CborValue::Integer(10.into())),
        ]))
        .unwrap();
    assert_eq!(main.get("name"), Some(&text("John")));
    assert!(main.get("bakery.name").is_none());
    assert!(has_many.is_empty());
    assert_eq!(has_one.len(), 1);
    let (relation, fk, child) = &has_one[0];
    assert_eq!(relation, "bakery");
    assert_eq!(fk, "bakery_id");
    assert_eq!(child.get("name"), Some(&text("New Bakery")));
    assert_eq!(
        child.get("profit_margin"),
        Some(&CborValue::Integer(10.into()))
    );
}

#[test]
fn bare_map_is_a_has_one_child() {
    let (_main, has_one, _has_many) = client_vista()
        .classify_insert(&record(&[("bakery", map(&[("name", text("New Bakery"))]))]))
        .unwrap();
    assert_eq!(has_one.len(), 1);
    assert_eq!(has_one[0].2.get("name"), Some(&text("New Bakery")));
}

#[test]
fn bare_list_is_a_has_many_group() {
    let (_main, has_one, has_many) = client_vista()
        .classify_insert(&record(&[(
            "orders",
            CborValue::Array(vec![
                map(&[("total", CborValue::Integer(1.into()))]),
                map(&[("total", CborValue::Integer(2.into()))]),
            ]),
        )]))
        .unwrap();
    assert!(has_one.is_empty());
    assert_eq!(has_many.len(), 1);
    let (relation, fk, children) = &has_many[0];
    assert_eq!(relation, "orders");
    assert_eq!(fk, "client_id");
    assert_eq!(children.len(), 2);
}

#[test]
fn bare_scalar_links_an_existing_record_via_the_foreign_key() {
    let (main, has_one, has_many) = client_vista()
        .classify_insert(&record(&[
            ("name", text("John")),
            ("bakery", text("bakery:hill_valley")),
        ]))
        .unwrap();
    assert!(has_one.is_empty());
    assert!(has_many.is_empty());
    assert!(main.get("bakery").is_none());
    assert_eq!(main.get("bakery_id"), Some(&text("bakery:hill_valley")));
}

/// Both forms of a has-one at once — link an existing row AND create
/// a child — is refused rather than resolved: the child insert would
/// silently overwrite the link.
#[test]
fn a_link_and_child_data_for_one_relation_is_an_error() {
    let scalar_and_dotted = client_vista().classify_insert(&record(&[
        ("bakery", text("bakery:hill_valley")),
        ("bakery.name", text("New Bakery")),
    ]));
    let err = scalar_and_dotted.unwrap_err().to_string();
    assert!(err.contains("both a record link and child data"), "{err}");

    // Same conflict spelled with the plain foreign-key column.
    let fk_and_map = client_vista().classify_insert(&record(&[
        ("bakery_id", text("b1")),
        ("bakery", map(&[("name", text("New Bakery"))])),
    ]));
    assert!(fk_and_map.is_err());

    // Either form ALONE stays legal.
    assert!(
        client_vista()
            .classify_insert(&record(&[("bakery", text("bakery:hill_valley"))]))
            .is_ok()
    );
    assert!(
        client_vista()
            .classify_insert(&record(&[("bakery", map(&[("name", text("New"))]))]))
            .is_ok()
    );
}

#[test]
fn has_many_given_a_scalar_is_an_error() {
    let err = client_vista().classify_insert(&record(&[("orders", text("order:1"))]));
    assert!(err.is_err());
}

#[test]
fn has_one_given_a_list_is_an_error() {
    let err =
        client_vista().classify_insert(&record(&[("bakery", CborValue::Array(vec![map(&[])]))]));
    assert!(err.is_err());
}

#[test]
fn has_many_given_a_map_is_an_error() {
    let err = client_vista().classify_insert(&record(&[("orders", map(&[("total", text("x"))]))]));
    assert!(err.is_err());
}

/// The children land in their targets, linked both ways.
#[tokio::test]
async fn nested_insert_writes_and_links_children() {
    use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet};

    let (bakeries, orders) = (MockShell::new(), MockShell::new());
    let clients = client_vista_over(bakeries.clone(), orders.clone());

    let id = clients
        .insert_return_id_value(&record(&[
            ("name", text("John")),
            ("bakery.name", text("Hill Valley")),
            (
                "orders",
                CborValue::Array(vec![map(&[("total", text("9"))])]),
            ),
        ]))
        .await
        .unwrap();

    let client = clients.get_value(&id).await.unwrap().unwrap();
    let bakery_id = client
        .get("bakery_id")
        .cloned()
        .expect("has-one link stamped");
    assert_eq!(bakeries.len(), 1);
    assert_eq!(
        Some(bakery_id),
        bakeries.record_ids().first().cloned().map(CborValue::Text)
    );
    let order = orders.get_record(&orders.record_ids()[0]).unwrap();
    assert_eq!(order.get("client_id"), Some(&text(&id)));
}

/// A contained relation's host column is plain data on insert.
#[test]
fn a_contained_relation_is_a_plain_field() {
    let metadata = VistaMetadata::new().with_reference(Reference::contains_many("lines", "lines"));
    let vista = Vista::new("order", MockShell::new().with_metadata(metadata));
    let lines = CborValue::Array(vec![map(&[("qty", CborValue::Integer(1.into()))])]);
    let (main, has_one, has_many) = vista
        .classify_insert(&record(&[("lines", lines.clone())]))
        .unwrap();
    assert_eq!(main.get("lines"), Some(&lines));
    assert!(has_one.is_empty() && has_many.is_empty());
}
