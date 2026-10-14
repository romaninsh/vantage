//! A spec's `references:` lowered into the vista metadata.

use vantage_vista::{JoinKey, ReferenceKind, VistaFactory};

use super::{parse, registry_resolver, test_db};
use crate::vista::SurrealVistaFactory;

#[test]
fn build_from_spec_surfaces_references_in_metadata() {
    let yaml = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
  primary_product:
    table: product
    kind: has_one
    foreign_key: primary_product
"#;
    let spec = parse(yaml);
    let factory = SurrealVistaFactory::new(test_db());
    let vista = factory.build_from_spec(spec).expect("build");

    let mut listed: Vec<&str> = vista.references().keys().map(String::as_str).collect();
    listed.sort();
    assert_eq!(listed, vec!["primary_product", "products"]);

    let products = vista.reference("products").expect("products ref");
    assert_eq!(products.target, "product");
    assert_eq!(products.kind, ReferenceKind::HasMany);
    assert_eq!(products.keys, vec![JoinKey::new("id", "bakery")]);

    let primary = vista.reference("primary_product").expect("primary ref");
    assert_eq!(primary.kind, ReferenceKind::HasOne);
    assert_eq!(primary.keys, vec![JoinKey::to_target_id("primary_product")]);
}

/// A `base:` vista registers only the relations it inherits, so a reference
/// it declares itself has no traversal target: a plain build error, not a
/// not-found.
#[test]
fn a_spec_reference_the_built_table_lacks_is_a_plain_error() {
    let bakery_spec = parse(
        r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
"#,
    );
    let view_spec = parse(
        r#"
name: bakery_view
columns: {}
surreal:
  base: bakery
  inherit:
    columns: [id, name]
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery
"#,
    );
    let resolver = registry_resolver(vec![("bakery".into(), bakery_spec)]);
    let err = SurrealVistaFactory::new(test_db())
        .with_resolver(resolver)
        .build_from_spec(view_spec)
        .map(drop)
        .expect_err("an unregistered spec reference must fail");
    assert!(!err.is_not_found(), "{err:?}");
    assert!(err.to_string().contains("absent"), "{err}");
}

/// A column-level `references:` reaches the metadata as a has-one keyed by
/// its column, its target named by the spec.
#[test]
fn column_reference_reaches_the_metadata() {
    let tag_spec = parse(
        r#"
name: tag
columns:
  id: { type: string, flags: [id] }
  batch: { type: string, references: batch }
"#,
    );
    let tag = SurrealVistaFactory::new(test_db())
        .build_from_spec(tag_spec)
        .expect("build tag");
    let batch = tag.reference("batch").expect("batch ref");
    assert_eq!(batch.kind, ReferenceKind::HasOne);
    assert_eq!(batch.target, "batch");
    assert_eq!(batch.keys, vec![JoinKey::to_target_id("batch")]);
}
