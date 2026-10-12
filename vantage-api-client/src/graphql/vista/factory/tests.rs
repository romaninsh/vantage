use super::*;
use vantage_types::EmptyEntity;

fn column_names(vista: &Vista) -> Vec<&str> {
    vista.columns().keys().map(String::as_str).collect()
}

fn vista_from(yaml: &str) -> Vista {
    let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    GraphqlApi::new("https://api.test/graphql")
        .vista_factory()
        .build_from_spec(spec)
        .unwrap()
}

#[test]
fn factory_builds_vista_with_metadata_and_driver_name() {
    let api = GraphqlApi::new("https://api.test/graphql");
    let table: Table<GraphqlApi, EmptyEntity> = Table::new("launches", api.clone())
        .with_id_column("id")
        .with_column_of::<String>("mission_name")
        .with_column_of::<i64>("launch_year");

    let vista = api.vista_factory().from_table(table).unwrap();
    assert_eq!(vista.name(), "launches");

    // Metadata carries every declared column.
    let cols = column_names(&vista);
    assert!(cols.contains(&"id"));
    assert!(cols.contains(&"mission_name"));
    assert!(cols.contains(&"launch_year"));
    assert_eq!(vista.id_column(), Some("id"));

    // Driver name surfaces from the shell so generic UI code can
    // distinguish a GraphQL-backed Vista from a REST one.
    assert_eq!(vista.driver_name(), "graphql");
}

#[test]
fn factory_capabilities_advertise_no_writes() {
    let api = GraphqlApi::new("https://api.test/graphql");
    let table: Table<GraphqlApi, EmptyEntity> =
        Table::new("launches", api.clone()).with_id_column("id");
    let vista = api.vista_factory().from_table(table).unwrap();
    let caps = vista.capabilities();
    assert!(caps.has(Capability::Traverse));
    // Writes are stubbed at the TableSource layer so the shell
    // doesn't advertise them, and the Vista refuses them.
    for write in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::Import,
        Capability::ConfineWrites,
    ] {
        assert!(!caps.has(write), "{write}");
    }
}

#[test]
fn build_from_yaml_spec_with_all_overrides() {
    // YAML covering: id column, multiple types, flags (id, title,
    // hidden), explicit graphql block with root_field, dialect, and
    // filter_arg overrides.
    let yaml = r#"
name: launches
columns:
  id:
    type: string
    flags: [id]
  mission_name:
    type: string
    flags: [title]
  launch_year:
    type: int
  is_tentative:
    type: bool
  internal_score:
    type: float
    flags: [hidden]
graphql:
  root_field: launches
  dialect: generic
  filter_arg: find
"#;
    let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let api = GraphqlApi::new("https://api.spacex.land/graphql/");
    let vista = api.vista_factory().build_from_spec(spec).unwrap();

    assert_eq!(vista.name(), "launches");
    assert_eq!(vista.driver_name(), "graphql");
    assert_eq!(vista.id_column(), Some("id"));

    let cols = column_names(&vista);
    for col in [
        "id",
        "mission_name",
        "launch_year",
        "is_tentative",
        "internal_score",
    ] {
        assert!(cols.contains(&col), "{col}");
    }

    // Title flag survives the YAML → Table → Vista round-trip.
    assert!(vista.title_columns().contains(&"mission_name"));
}

#[test]
fn build_from_yaml_spec_minimal_uses_defaults() {
    // No graphql block, no explicit id_column — id flag does the work.
    let vista = vista_from(
        r#"
name: rockets
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
"#,
    );
    assert_eq!(vista.name(), "rockets");
    assert_eq!(vista.id_column(), Some("id"));
}

fn expect_err(result: Result<Vista>, needle: &str) {
    match result {
        Ok(_) => panic!("expected error containing {needle:?}"),
        Err(e) => assert!(
            e.to_string().contains(needle),
            "error {:?} did not contain {needle:?}",
            e.to_string()
        ),
    }
}

fn build(yaml: &str) -> Result<Vista> {
    let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    GraphqlApi::new("https://api.test/graphql")
        .vista_factory()
        .build_from_spec(spec)
}

#[test]
fn build_from_yaml_spec_rejects_unknown_type() {
    let yaml = r#"
name: things
columns:
  id:
    type: string
    flags: [id]
  weird:
    type: complex
"#;
    expect_err(build(yaml), "Unknown YAML column type");
}

#[test]
fn build_from_yaml_spec_rejects_missing_id_column() {
    // id_column points at a column that isn't in `columns:`.
    let yaml = r#"
name: things
id_column: missing
columns:
  name:
    type: string
"#;
    expect_err(build(yaml), "id column not present");
}

#[test]
fn build_from_yaml_spec_rejects_multi_key_references() {
    let yaml = r#"
name: deployments
columns:
  id:
    type: string
    flags: [id]
references:
  release:
    table: releases
    keys:
      - { to: product_id, from: product_id }
      - { to: version_id, from: version_id }
"#;
    expect_err(build(yaml), "multi-column `keys:` are not supported");
}

#[test]
fn yaml_spec_with_unknown_dialect_fails_to_parse() {
    let yaml = r#"
name: things
columns:
  id:
    type: string
    flags: [id]
graphql:
  dialect: relay
"#;
    let err = serde_yaml_ng::from_str::<GraphqlApiVistaSpec>(yaml)
        .unwrap_err()
        .to_string();
    assert!(err.contains("relay"), "{err}");
}

#[test]
fn column_field_alias_must_be_a_plain_name() {
    let yaml = r#"
name: runs
columns:
  id:
    type: string
    flags: [id]
  run_id:
    type: string
    graphql:
      field: run.id
"#;
    expect_err(build(yaml), "dotted paths are not supported");
    let yaml = yaml.replace("run.id", "\"run id\"");
    expect_err(build(&yaml), "GraphQL name is not valid");
}

/// A generic-dialect table can't spell `order_by` or `_ilike`, so it
/// must say so and let the consumer sort and search locally.
#[test]
fn generic_dialect_advertises_client_side_sort_and_search() {
    let vista = vista_from(
        "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  dialect: generic\n",
    );
    let caps = vista.capabilities();
    assert!(!caps.has(Capability::Order));
    assert!(!caps.has(Capability::Search));
    assert!(!caps.has(Capability::FilterOps));
}

/// Hasura can, so the same table on that dialect pushes the work down.
#[test]
fn hasura_dialect_advertises_server_side_sort_and_search() {
    let vista = vista_from(
        "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  dialect: hasura\n",
    );
    let caps = vista.capabilities();
    assert!(caps.has(Capability::Order));
    assert!(caps.has(Capability::Search));
    assert!(caps.has(Capability::FilterOps));
}

/// `filter: false` is for a root field that takes no arguments at all;
/// nothing derived from a condition may be advertised — paging
/// included, since local filtering would cut server pages short.
#[test]
fn filter_false_disables_everything_condition_shaped() {
    let vista = vista_from(
        "name: stacks\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  dialect: hasura\n  filter: false\n  paginate: true\n",
    );
    let caps = vista.capabilities();
    assert!(!caps.has(Capability::Search));
    assert!(!caps.has(Capability::FilterOps));
    assert!(!caps.has(Capability::Window));
}

/// GraphQL paging is offset paging, so a page-number continuation is no
/// real cursor: `Cursor` is never advertised.
#[test]
fn paging_advertises_a_window_but_no_cursor() {
    let vista = vista_from(
        "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  paginate: true\n",
    );
    let caps = vista.capabilities();
    assert!(caps.has(Capability::Window));
    assert!(!caps.has(Capability::Cursor));
}

/// The YAML `searchable` flag reaches the Vista column.
#[test]
fn searchable_flag_carries_over() {
    let vista = vista_from(
        "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\n  details:\n    type: string\n    flags: [searchable]\n",
    );
    let col = vista.column("details").unwrap();
    assert!(col.has_flag(ColumnFlag::Searchable));
}

/// A spec reference carries its join key: a has-many matches the parent's
/// id to the target's foreign key, a has-one the parent's foreign key to
/// the target's id.
#[test]
fn spec_references_carry_their_join_keys() {
    let vista = vista_from(
        "name: client\ncolumns:\n  id:\n    type: string\n    flags: [id]\n  boss_id:\n    type: string\nreferences:\n  orders:\n    table: order\n    kind: has_many\n    foreign_key: client_id\n  boss:\n    table: client\n    kind: has_one\n    foreign_key: boss_id\n",
    );
    let orders = vista.reference("orders").unwrap();
    assert_eq!(
        orders.keys,
        vec![vantage_vista::JoinKey::new("id", "client_id")]
    );
    let boss = vista.reference("boss").unwrap();
    assert_eq!(
        boss.keys,
        vec![vantage_vista::JoinKey::to_target_id("boss_id")]
    );
}
