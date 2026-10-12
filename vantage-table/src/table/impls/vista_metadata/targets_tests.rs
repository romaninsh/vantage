use indexmap::IndexMap;
use vantage_types::EmptyEntity;
use vantage_vista::{ReferenceSpec, VistaMetadata};

use crate::mocks::{MockReference, mock_table_source::MockTableSource};
use crate::references::Cardinality;
use crate::table::{Table, VistaMetadataOptions};

/// `orders` (has-many) and `bakery` (has-one), each building a table named
/// `target_table`.
fn client() -> Table<MockTableSource, EmptyEntity> {
    let mut table = Table::new("clients", MockTableSource::new()).with_id_column("id");
    table.add_ref(
        "orders",
        Box::new(MockReference::new(Cardinality::HasMany, "client_id")),
    );
    table.add_ref(
        "bakery",
        Box::new(MockReference::new(Cardinality::HasOne, "bakery_id")),
    );
    table
}

fn metadata(table: &Table<MockTableSource, EmptyEntity>) -> VistaMetadata {
    table.vista_metadata(VistaMetadataOptions {
        references: true,
        ..VistaMetadataOptions::default()
    })
}

fn spec_refs(json: &str) -> IndexMap<String, ReferenceSpec> {
    serde_json::from_str(json).unwrap()
}

fn target<'a>(metadata: &'a VistaMetadata, relation: &str) -> &'a str {
    &metadata.references[relation].target
}

#[test]
fn join_targets_take_the_built_table_name() {
    let metadata = metadata(&client());
    assert_eq!(target(&metadata, "orders"), "target_table");
    assert_eq!(target(&metadata, "bakery"), "target_table");
}

#[test]
fn spec_targets_take_the_declared_name_and_script() {
    let table = client();
    let mut metadata = metadata(&table);
    let refs = spec_refs(r#"{"orders": {"table": "order", "kind": "has_many"}}"#);
    table
        .name_spec_targets(&mut metadata, &refs, |_| Some("table(\"order\")".into()))
        .unwrap();
    assert_eq!(target(&metadata, "orders"), "order");
    assert!(metadata.references["orders"].build_script.is_some());
    // A relation the spec doesn't declare is named by its built target.
    assert_eq!(target(&metadata, "bakery"), "target_table");
}

#[test]
fn a_spec_reference_the_table_lacks_is_a_plain_error() {
    let table = client();
    let mut metadata = metadata(&table);
    let refs = spec_refs(r#"{"ghost": {"table": "ghost"}}"#);
    let err = table
        .name_spec_targets(&mut metadata, &refs, |_| None)
        .unwrap_err();
    assert!(!err.is_not_found(), "{err:?}");
}
