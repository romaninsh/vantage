//! `GraphqlApiVistaFactory`: Vistas from typed tables and from YAML specs,
//! what they advertise, and the specs it refuses at build time.

mod support;

use std::sync::Arc;

use serde_json::json;
use support::references::{answer, row, texts};
use support::{mount, requests, test_client};
use vantage_api_client::{
    CountMode, FilterDialect, GraphqlApi, GraphqlApiVistaFactory, GraphqlApiVistaSpec, Supports,
};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{Capability, ColumnFlag, SpecResolver, Vista, VistaFactory};
use wiremock::MockServer;

fn api(server: &MockServer) -> GraphqlApi {
    GraphqlApi::builder(server.uri())
        .http_client(test_client())
        .build()
}

fn build(api: &GraphqlApi, yaml: &str) -> vantage_core::Result<Vista> {
    let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    api.vista_factory().build_from_spec(spec)
}

fn vista_from(api: &GraphqlApi, yaml: &str) -> Vista {
    build(api, yaml).unwrap()
}

const ID_ONLY: &str = "name: launches\ncolumns:\n  id:\n    type: string\n    flags: [id]\n";

// ── From a typed table ───────────────────────────────────────────────

fn typed_launches(api: GraphqlApi) -> Table<GraphqlApi, EmptyEntity> {
    Table::<GraphqlApi, EmptyEntity>::new("launches", api)
        .with_id_column("id")
        .with_column_of::<String>("mission_name")
        .with_column_of::<i64>("launch_year")
}

#[tokio::test]
async fn a_typed_table_becomes_a_vista_carrying_its_columns_and_id() {
    let server = MockServer::start().await;
    let api = api(&server);
    let vista = api
        .vista_factory()
        .from_table(typed_launches(api.clone()))
        .unwrap();

    assert_eq!(vista.name(), "launches");
    assert_eq!(vista.id_column(), Some("id"));
    let columns: Vec<&str> = vista.columns().keys().map(String::as_str).collect();
    assert_eq!(columns, ["id", "mission_name", "launch_year"]);
    assert_eq!(vista.driver_name(), "graphql");
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn a_typed_tables_relations_survive_the_wrap() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches(find: {rocket_id: \"falcon9\"}) { id rocket_id } }",
        "launches",
        json!([{ "id": "l1", "rocket_id": "falcon9" }]),
    )
    .await;
    let api = api(&server);
    let rockets = Table::<GraphqlApi, EmptyEntity>::new("rockets", api.clone())
        .with_id_column("id")
        .with_many("launches", "rocket_id", |api| {
            Table::<GraphqlApi, EmptyEntity>::new("launches", api)
                .with_id_column("id")
                .with_column_of::<String>("rocket_id")
        });
    let vista = api.vista_factory().from_table(rockets).unwrap();
    assert!(vista.reference("launches").is_some());

    let launches = vista
        .get_ref("launches", Some(&row(&[("id", "falcon9")])))
        .unwrap();
    assert_eq!(
        texts(&launches.list_values().await.unwrap(), "rocket_id"),
        ["falcon9"]
    );
}

#[test]
fn a_hasura_tables_columns_are_orderable_except_dotted_ones() {
    let api = GraphqlApi::builder("http://x/graphql")
        .dialect(FilterDialect::Hasura)
        .http_client(test_client())
        .build();
    let table = typed_launches(api.clone()).with_column_of::<String>("rocket.name");
    let vista = api.vista_factory().from_table(table).unwrap();
    assert!(
        vista
            .column("mission_name")
            .unwrap()
            .has_flag(ColumnFlag::Orderable)
    );
    assert!(
        !vista
            .column("rocket.name")
            .unwrap()
            .has_flag(ColumnFlag::Orderable)
    );

    let generic = GraphqlApi::new("http://x/graphql");
    let table = typed_launches(generic.clone());
    let vista = generic.vista_factory().from_table(table).unwrap();
    assert!(
        !vista
            .column("mission_name")
            .unwrap()
            .has_flag(ColumnFlag::Orderable)
    );
}

// ── From a YAML spec ─────────────────────────────────────────────────

#[test]
fn a_spec_carries_its_name_columns_id_and_title() {
    let api = GraphqlApi::new("https://api.spacex.land/graphql/");
    let vista = vista_from(
        &api,
        r#"
name: launches
columns:
  id: { type: string, flags: [id] }
  mission_name: { type: string, flags: [title] }
  launch_year: { type: int }
  is_tentative: { type: bool }
  internal_score: { type: float, flags: [hidden] }
graphql:
  root_field: launches
  dialect: generic
  filter_arg: find
"#,
    );
    assert_eq!(vista.name(), "launches");
    assert_eq!(vista.driver_name(), "graphql");
    assert_eq!(vista.id_column(), Some("id"));
    assert_eq!(vista.title_columns(), ["mission_name"]);
    let columns: Vec<&str> = vista.columns().keys().map(String::as_str).collect();
    assert_eq!(
        columns,
        [
            "id",
            "mission_name",
            "launch_year",
            "is_tentative",
            "internal_score"
        ]
    );
}

#[test]
fn every_documented_column_type_name_builds() {
    let api = GraphqlApi::new("http://x/graphql");
    let types = [
        "int",
        "integer",
        "i32",
        "bigint",
        "i64",
        "long",
        "float",
        "double",
        "f64",
        "f32",
        "bool",
        "boolean",
        "string",
        "text",
        "str",
        "id",
        "datetime",
        "timestamp",
        "timestamptz",
        "date",
        "time",
        "uuid",
        "json",
    ];
    for ty in types {
        let yaml = format!("{ID_ONLY}  other:\n    type: {ty}\n");
        assert!(build(&api, &yaml).is_ok(), "{ty}");
    }
}

#[test]
fn an_unknown_column_type_fails_the_build() {
    let api = GraphqlApi::new("http://x/graphql");
    let yaml = format!("{ID_ONLY}  weird:\n    type: complex\n");
    let err = build(&api, &yaml).expect_err("refused").to_string();
    assert!(err.contains("Unknown YAML column type"), "{err}");
}

#[test]
fn an_id_column_the_spec_does_not_declare_fails_the_build() {
    let api = GraphqlApi::new("http://x/graphql");
    let yaml = "name: things\nid_column: missing\ncolumns:\n  name:\n    type: string\n";
    assert!(build(&api, yaml).is_err());
}

#[test]
fn a_column_name_that_is_not_a_graphql_name_fails_the_build() {
    let api = GraphqlApi::new("http://x/graphql");
    let yaml = format!("{ID_ONLY}  \"rocket name\":\n    type: string\n");
    let err = build(&api, &yaml).expect_err("refused").to_string();
    assert!(err.contains("GraphQL name is not valid"), "{err}");
}

#[test]
fn multi_column_keys_are_refused_naming_the_relation() {
    let api = GraphqlApi::new("http://x/graphql");
    let yaml = r#"
name: deployments
columns:
  id: { type: string, flags: [id] }
references:
  release:
    table: releases
    keys:
      - { to: product_id, from: product_id }
      - { to: version_id, from: version_id }
"#;
    let err = build(&api, yaml).expect_err("refused");
    assert!(err.is_unsupported(), "{err}");
    assert!(err.to_string().contains("release"), "{err}");
}

#[test]
fn a_dialect_other_than_generic_or_hasura_does_not_parse() {
    let yaml = format!("{ID_ONLY}graphql:\n  dialect: relay\n");
    let err = serde_yaml_ng::from_str::<GraphqlApiVistaSpec>(&yaml)
        .unwrap_err()
        .to_string();
    assert!(err.contains("relay"), "{err}");
}

#[test]
fn an_unknown_key_in_the_graphql_block_does_not_parse() {
    let yaml = format!("{ID_ONLY}graphql:\n  pagination: true\n");
    assert!(serde_yaml_ng::from_str::<GraphqlApiVistaSpec>(&yaml).is_err());
}

#[test]
fn the_searchable_flag_reaches_the_vista_column() {
    let api = GraphqlApi::new("http://x/graphql");
    let yaml = format!("{ID_ONLY}  details:\n    type: string\n    flags: [searchable]\n");
    let vista = vista_from(&api, &yaml);
    assert!(
        vista
            .column("details")
            .unwrap()
            .has_flag(ColumnFlag::Searchable)
    );
}

#[tokio::test]
async fn a_block_names_the_root_field_args_and_response_path() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { ordersList(input: {}) { edges { node { id } } } }",
        "ordersList",
        json!({ "edges": [{ "node": { "id": "o1" } }] }),
    )
    .await;
    let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(
        "name: orders\ncolumns:\n  id:\n    type: string\n    flags: [id]\ngraphql:\n  root_field: ordersList\n  args: { input: {} }\n  response_path: edges.node\n",
    )
    .unwrap();
    let factory = api(&server).vista_factory();

    let vista = factory.build_from_spec(spec.clone()).unwrap();
    assert_eq!(vista.list_values().await.unwrap().len(), 1);
    let table = factory.table_from_spec(&spec).unwrap();
    assert_eq!(table.table_name(), "ordersList");
}

#[test]
fn a_bad_name_in_the_graphql_block_fails_the_build() {
    let api = GraphqlApi::new("http://x/graphql");
    for block in [
        "root_field: \"a b\"",
        "filter_arg: \"find)\"",
        "count_field: \"x y\"",
        "args: { \"in put\": 1 }",
        "response_path: \"edges.no de\"",
    ] {
        let yaml = format!("{ID_ONLY}graphql:\n  {block}\n");
        let err = build(&api, &yaml).expect_err("refused").to_string();
        assert!(err.contains("GraphQL name is not valid"), "{block}: {err}");
    }
}

// ── What a Vista advertises ──────────────────────────────────────────

fn capabilities(api: &GraphqlApi, graphql: &str) -> Vec<Capability> {
    let yaml = format!("{ID_ONLY}graphql:\n{graphql}");
    let vista = vista_from(api, &yaml);
    [
        Capability::Traverse,
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::Window,
        Capability::Cursor,
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::Import,
        Capability::ConfineWrites,
    ]
    .into_iter()
    .filter(|c| vista.capabilities().has(*c))
    .collect()
}

/// Generic leaves sort and search to the consumer; Hasura pushes them and
/// the operators down; paging adds a window but never a cursor; nothing
/// ever writes. `filter: false` switches off everything condition-shaped,
/// and order and search can be switched off per table.
#[test]
fn a_vista_advertises_what_its_block_pushes_down() {
    let api = GraphqlApi::new("http://x/graphql");
    assert_eq!(
        capabilities(&api, "  dialect: generic\n"),
        [Capability::Traverse]
    );
    assert_eq!(
        capabilities(&api, "  dialect: hasura\n"),
        [
            Capability::Traverse,
            Capability::Order,
            Capability::Search,
            Capability::FilterOps
        ]
    );
    assert_eq!(
        capabilities(&api, "  dialect: hasura\n  paginate: true\n"),
        [
            Capability::Traverse,
            Capability::Order,
            Capability::Search,
            Capability::FilterOps,
            Capability::Window
        ]
    );
    assert_eq!(
        capabilities(
            &api,
            "  dialect: hasura\n  filter: false\n  paginate: true\n"
        ),
        [Capability::Traverse, Capability::Order]
    );
    assert_eq!(
        capabilities(&api, "  dialect: hasura\n  order: false\n  search: false\n"),
        [Capability::Traverse, Capability::FilterOps]
    );
}

#[test]
fn a_block_overrides_the_apis_supports_key_by_key() {
    let api = GraphqlApi::builder("http://x/graphql")
        .supports(
            Supports::default()
                .with_paginate(true)
                .with_count(CountMode::Aggregate),
        )
        .build();
    let silent = capabilities(&api, "  root_field: launchesPast\n");
    assert!(
        silent.contains(&Capability::Window),
        "the builder's paginate stays"
    );

    let overridden = capabilities(&api, "  paginate: false\n");
    assert!(!overridden.contains(&Capability::Window));
}

#[tokio::test]
async fn a_block_silent_on_count_keeps_the_apis_count_mode() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        200,
        r#"{"data":{"launches_aggregate":{"aggregate":{"count":9}}}}"#,
    )
    .await;
    let api = GraphqlApi::builder(server.uri())
        .dialect(FilterDialect::Hasura)
        .supports(Supports::default().with_count(CountMode::Aggregate))
        .http_client(test_client())
        .build();
    let vista = vista_from(
        &api,
        &format!("{ID_ONLY}graphql:\n  root_field: launches\n"),
    );
    assert_eq!(vista.count().await.unwrap(), 9);
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn a_lazy_column_is_computed_from_each_row_and_never_selected() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches { id name } }",
        "launches",
        json!([{ "id": "1", "name": "ab" }]),
    )
    .await;
    let vista = vista_from(
        &api(&server),
        "name: launches\ncolumns:\n  id: { type: string, flags: [id] }\n  name: { type: string }\n  shout: { type: string, lazy: \"row.name.to_upper()\" }\n",
    );
    let rows = vista.list_values().await.unwrap();
    assert_eq!(
        rows["1"].get("shout"),
        Some(&ciborium::Value::Text("AB".into()))
    );
}

// ── Registry and resolver ────────────────────────────────────────────

#[test]
fn a_registered_spec_builds_by_name_and_an_unknown_name_is_not_found() {
    let api = GraphqlApi::new("http://x/graphql");
    let mut factory = api.vista_factory();
    factory.register_yaml(ID_ONLY).unwrap();
    assert_eq!(factory.build("launches").unwrap().name(), "launches");

    let err = factory.build("ghost").expect_err("unknown");
    assert!(err.is_not_found(), "{err}");
    assert!(err.to_string().contains("No registered spec"), "{err}");
}

#[test]
fn registering_yaml_that_does_not_parse_is_an_error() {
    let mut factory = GraphqlApi::new("http://x/graphql").vista_factory();
    let err = factory.register_yaml("name: [").unwrap_err().to_string();
    assert!(err.contains("Failed to parse"), "{err}");
}

#[test]
fn the_factory_reports_its_api() {
    let api = GraphqlApi::new("http://x/graphql");
    let factory = GraphqlApiVistaFactory::new(api);
    assert_eq!(factory.api().endpoint(), "http://x/graphql");
}

#[tokio::test]
async fn a_resolver_replaces_the_registry_for_reference_targets() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { clients(where: {id: {_eq: \"c2\"}}) { id name } }",
        "clients",
        json!([{ "id": "c2", "name": "Doc" }]),
    )
    .await;
    let order: GraphqlApiVistaSpec = serde_yaml_ng::from_str(
        "name: order\ncolumns:\n  id: { type: string, flags: [id] }\n  client_id: { type: string }\ngraphql:\n  dialect: hasura\nreferences:\n  client: { table: client, kind: has_one, foreign_key: client_id }\n",
    )
    .unwrap();
    let client: GraphqlApiVistaSpec = serde_yaml_ng::from_str(
        "name: client\ncolumns:\n  id: { type: string, flags: [id] }\n  name: { type: string }\ngraphql:\n  root_field: clients\n  dialect: hasura\n",
    )
    .unwrap();
    let resolver: Arc<dyn SpecResolver<GraphqlApiVistaSpec>> =
        Arc::new(move |name: &str| (name == "client").then(|| client.clone()));
    let factory = api(&server).vista_factory().with_resolver(resolver);
    let orders = factory.build_from_spec(order).unwrap();

    let target = orders
        .get_ref("client", Some(&row(&[("id", "o3"), ("client_id", "c2")])))
        .unwrap();
    assert_eq!(texts(&target.list_values().await.unwrap(), "name"), ["Doc"]);
}
