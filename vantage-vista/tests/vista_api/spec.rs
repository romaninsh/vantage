use std::sync::Arc;

use vantage_vista::{
    ColumnFlag, ColumnSpec, JoinKey, NoExtras, ReferenceKind, ReferenceSugar, SpecResolver,
    VistaSpec, resolve_base_spec,
};

type Spec = VistaSpec<NoExtras, NoExtras, NoExtras>;

fn spec(yaml: &str) -> Spec {
    serde_yaml_ng::from_str(yaml).unwrap()
}

#[test]
fn a_new_spec_declares_only_its_name() {
    let spec = Spec::new("client");
    assert_eq!(spec.name, "client");
    assert!(spec.columns.is_empty());
    assert!(spec.references.is_empty());
    assert!(spec.contained.is_empty());
    assert_eq!(spec.id_column, None);
    assert_eq!(spec.datasource, None);
}

#[test]
fn a_minimal_spec_parses_with_typed_columns_and_flags() {
    let spec = spec(
        "name: client\ndatasource: main\nid_column: code\ncolumns:\n  code:\n    type: String\n    flags: [id, searchable]\n  note: {}\n",
    );
    assert_eq!(spec.datasource.as_deref(), Some("main"));
    assert_eq!(spec.id_column.as_deref(), Some("code"));
    let code = &spec.columns["code"];
    assert_eq!(code.col_type.as_deref(), Some("String"));
    assert_eq!(code.flags, [ColumnFlag::Id, ColumnFlag::Searchable]);
    assert!(code.has_flag(ColumnFlag::Id));
    assert!(!code.has_flag(ColumnFlag::Hidden));
    assert_eq!(spec.columns["note"].col_type, None);
}

#[test]
fn an_unknown_field_in_an_extras_block_is_rejected() {
    let err =
        serde_yaml_ng::from_str::<ContainedProbe>("host_column: lines\nbogus: 1\ncolumns: {}\n")
            .unwrap_err();
    assert!(err.to_string().contains("bogus"), "{err}");
}

type ContainedProbe = vantage_vista::ContainedYaml<NoExtras>;

#[test]
fn a_contained_section_parses_into_contained_relations() {
    let spec = spec(
        "name: order\ncolumns: {}\ncontained:\n  lines:\n    host_column: lines\n    kind: contains_many\n    id_column: sku\n    columns:\n      sku: {type: String}\n  note:\n    host_column: note\n    columns: {}\n",
    );
    let lines = &spec.contained["lines"];
    assert_eq!(lines.kind, ReferenceKind::ContainsMany);
    assert_eq!(lines.host_column, "lines");
    assert_eq!(lines.id_column.as_deref(), Some("sku"));
    assert!(lines.columns.contains_key("sku"));
    assert_eq!(spec.contained["note"].kind, ReferenceKind::ContainsOne);

    let yaml = serde_yaml_ng::to_string(&spec).unwrap();
    let back: Spec = serde_yaml_ng::from_str(&yaml).unwrap();
    assert_eq!(back.contained["lines"].kind, ReferenceKind::ContainsMany);
}

#[test]
fn the_reference_sugar_and_the_full_form_both_parse() {
    let spec = spec(
        "name: order\ncolumns:\n  product:\n    references: products\n  client:\n    references:\n      table: clients\n      kind: has_one\n      foreign_key: client_id\n",
    );
    assert!(matches!(
        spec.columns["product"].references,
        Some(ReferenceSugar::Sugar(ref table)) if table == "products"
    ));
    assert!(matches!(
        spec.columns["client"].references,
        Some(ReferenceSugar::Full(ref full)) if full.table == "clients"
    ));
}

#[cfg(feature = "rhai")]
#[test]
fn a_lazy_column_keeps_its_declared_type_or_none() {
    let typed = ColumnSpec::<NoExtras>::new()
        .with_type("i64")
        .with_flag("hidden")
        .with_lazy("1 + 1");
    let column = typed.lazy_column("n").unwrap().unwrap();
    assert_eq!(column.original_type.as_deref(), Some("i64"));
    assert!(column.is_computed());
    assert!(column.has_flag(ColumnFlag::Hidden));
    assert!(column.has_flag(ColumnFlag::Calculated));

    let untyped = ColumnSpec::<NoExtras>::new().with_lazy("2");
    assert_eq!(
        untyped.lazy_column("n").unwrap().unwrap().original_type,
        None
    );

    assert!(
        ColumnSpec::<NoExtras>::new()
            .lazy_column("n")
            .unwrap()
            .is_none()
    );
    assert!(
        ColumnSpec::<NoExtras>::new()
            .with_lazy("1 +")
            .lazy_column("n")
            .is_err()
    );
    let with_expr = ColumnSpec::<NoExtras>::new().with_expr("x");
    assert_eq!(with_expr.expr.as_deref(), Some("x"));
}

#[test]
fn the_id_column_is_explicit_then_flagged_then_the_drivers_default() {
    let explicit = spec("name: a\nid_column: code\ncolumns: { key: { flags: [id] } }");
    assert_eq!(explicit.resolve_id_column("_id"), "code");
    let flagged = spec("name: a\ncolumns: { name: {}, key: { flags: [id] } }");
    assert_eq!(flagged.resolve_id_column("_id"), "key");
    let bare = spec("name: a\ncolumns: { name: {} }");
    assert_eq!(bare.resolve_id_column("id"), "id");
    assert_eq!(bare.resolve_id_column("_id"), "_id");
}

#[test]
fn a_foreign_key_defaults_to_the_relation_name() {
    let spec = spec(
        "name: a\ncolumns: {}\nreferences:\n  owner: { table: users, foreign_key: owner_id }\n  team: { table: teams }",
    );
    assert_eq!(spec.references["owner"].foreign_key("owner"), "owner_id");
    assert_eq!(spec.references["team"].foreign_key("team"), "team");
    assert_eq!(spec.references["team"].kind, ReferenceKind::HasOne);
}

#[test]
fn a_reference_spec_lowers_to_a_join() {
    let spec = spec(
        "name: a\ncolumns: {}\nreferences:\n  owner: { table: users, foreign_key: owner_id }\n  orders: { table: orders, kind: has_many }\n  deploys: { table: deployments, kind: has_many, keys: [{ from: product_id, to: product }, { from: version_id, to: version }] }\n  lines: { table: lines, kind: contains_many }",
    );
    let owner = spec.references["owner"]
        .to_reference("owner", "id")
        .unwrap();
    assert_eq!(owner.kind, ReferenceKind::HasOne);
    assert_eq!(owner.target, "users");
    assert_eq!(owner.keys, [JoinKey::to_target_id("owner_id")]);

    let orders = spec.references["orders"]
        .to_reference("orders", "code")
        .unwrap();
    assert_eq!(orders.keys, [JoinKey::new("code", "orders")]);

    let deploys = spec.references["deploys"]
        .to_reference("deploys", "id")
        .unwrap();
    assert_eq!(
        deploys.keys,
        [
            JoinKey::new("product_id", "product"),
            JoinKey::new("version_id", "version")
        ]
    );
    assert!(
        spec.references["lines"]
            .to_reference("lines", "id")
            .is_err()
    );
}

#[test]
fn a_base_spec_needs_a_resolver_that_knows_it() {
    let none: Option<Arc<dyn SpecResolver<u8>>> = None;
    assert!(resolve_base_spec(none, "b").is_err());

    let resolver: Arc<dyn SpecResolver<u8>> = Arc::new(|name: &str| (name == "b").then_some(7));
    assert!(resolve_base_spec(Some(resolver.clone()), "c").is_err());
    assert_eq!(resolve_base_spec(Some(resolver.clone()), "b").unwrap().1, 7);
    assert_eq!(resolver.resolve("b"), Some(7));
}
