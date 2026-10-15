//! `GraphqlSelect` as a `Selectable` and `GraphqlApi` as a
//! `SelectableDataSource`: the generic query-builder surface over the same
//! GraphQL documents.

mod support;

use serde_json::json;
use support::{mount, requests, test_client};
use vantage_api_client::{
    AnyGraphqlType, FilterDialect, GraphqlApi, GraphqlCondition, GraphqlSelect,
};
use vantage_expressions::traits::datasource::SelectableDataSource;
use vantage_expressions::{Expression, Expressive, Order, Selectable};
use vantage_table::column::core::Column;
use wiremock::MockServer;

#[tokio::test]
async fn the_builder_methods_fill_the_select() {
    let mut select = GraphqlSelect::new();
    select.add_source("launches", None);
    select.add_field("id");
    select.add_field("mission_name");
    select.add_where_condition(GraphqlCondition::eq("year", 2018));
    select.add_order_by(GraphqlCondition::eq("mission_name", "_"), Order::Desc);
    select.set_limit(Some(5), Some(10));

    assert!(select.has_fields());
    assert!(select.has_where_conditions());
    assert!(select.has_order_by());
    assert_eq!(select.get_limit(), Some(5));
    assert_eq!(select.get_skip(), Some(10));
    assert_eq!(
        select.preview(),
        "query { launches(find: {year: 2018}, limit: 5, offset: 10) { id mission_name } }"
    );
}

#[tokio::test]
async fn a_hasura_select_renders_the_sort_the_builder_added() {
    let mut select = GraphqlSelect::new().with_dialect(FilterDialect::Hasura);
    select.add_source("users", None);
    select.add_field("id");
    select.add_order_by(GraphqlCondition::eq("created_at", "_"), Order::Desc);
    assert_eq!(
        select.render().await.unwrap().query,
        "query { users(order_by: [{created_at: desc}]) { id } }"
    );
}

#[test]
fn an_order_that_names_no_column_is_dropped() {
    let mut select = GraphqlSelect::new();
    select.add_order_by(
        GraphqlCondition::Or(vec![GraphqlCondition::eq("a", 1)]),
        Order::Asc,
    );
    assert!(!select.has_order_by());
}

#[test]
fn the_clear_methods_empty_what_they_name() {
    let mut select = GraphqlSelect::new();
    select.add_field("id");
    select.add_where_condition(GraphqlCondition::eq("a", 1));
    select.add_order_by(GraphqlCondition::eq("a", "_"), Order::Asc);
    select.clear_fields();
    select.clear_where_conditions();
    select.clear_order_by();
    assert!(!select.has_fields());
    assert!(!select.has_where_conditions());
    assert!(!select.has_order_by());
}

#[test]
fn grouping_and_distinct_have_no_graphql_spelling() {
    let mut select = GraphqlSelect::new();
    select.add_group_by(Expression::<AnyGraphqlType>::new("x", vec![]));
    select.set_distinct(true);
    assert!(!select.has_group_by());
    assert!(!select.is_distinct());
    select.clear_group_by();
    assert!(!select.has_group_by());
}

#[test]
fn an_expression_is_not_a_selectable_field() {
    let mut select = GraphqlSelect::new();
    select.add_expression(Expression::<AnyGraphqlType>::new("1 + 1", vec![]));
    assert!(!select.has_fields());
}

#[test]
fn a_select_is_an_expression_of_its_preview() {
    let mut select = GraphqlSelect::new();
    select.add_source("launches", None);
    select.add_field("id");
    let expr = select.expr();
    assert_eq!(expr.template, "query { launches { id } }");
    assert!(expr.parameters.is_empty());
}

#[test]
fn aggregates_are_placeholder_expressions_in_the_hasura_shape() {
    let mut select = GraphqlSelect::new();
    select.add_source("launches", None);
    assert_eq!(
        select.as_count().template,
        "launches_aggregate { aggregate { count } }"
    );

    let year = Column::<AnyGraphqlType>::new("year");
    for (expr, kind) in [
        (select.as_sum(year.clone()), "sum"),
        (select.as_max(year.clone()), "max"),
        (select.as_min(year), "min"),
    ] {
        assert!(
            expr.template
                .starts_with(&format!("launches_aggregate {{ aggregate {{ {kind} {{ ")),
            "{}",
            expr.template
        );
        assert!(expr.template.contains("year"), "{}", expr.template);
    }
    assert!(select.as_field("id").template.contains("launches"));
}

// ── SelectableDataSource ─────────────────────────────────────────────

fn api(server: &MockServer) -> GraphqlApi {
    GraphqlApi::builder(server.uri())
        .http_client(test_client())
        .build()
}

fn select_launches(api: &GraphqlApi) -> GraphqlSelect {
    let mut select = api.select();
    select.add_source("launches", None);
    api.add_select_column(
        &mut select,
        Expression::new("ignored", vec![]),
        Some("mission_name"),
    );
    api.add_select_column(&mut select, Expression::new("dropped", vec![]), None);
    select
}

#[tokio::test]
async fn the_data_source_hands_out_an_empty_select_and_records_aliased_columns() {
    let server = MockServer::start().await;
    let api = api(&server);
    assert!(api.select().root_field().is_none());
    assert_eq!(select_launches(&api).fields(), ["mission_name"]);
}

#[tokio::test]
async fn executing_a_select_returns_the_rows_as_flat_values() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        200,
        r#"{"data":{"launches":[{"mission_name":"a"},{"mission_name":"b"}]}}"#,
    )
    .await;
    let api = api(&server);
    let rows = api.execute_select(&select_launches(&api)).await.unwrap();
    let values: Vec<_> = rows.iter().map(|r| r.value().clone()).collect();
    assert_eq!(
        values,
        [
            json!({ "mission_name": "a" }),
            json!({ "mission_name": "b" })
        ]
    );
}

#[tokio::test]
async fn a_single_object_is_one_row_and_null_is_none() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        200,
        r#"{"data":{"launches":{"mission_name":"a"}}}"#,
    )
    .await;
    let api1 = api(&server);
    let rows = api1.execute_select(&select_launches(&api1)).await.unwrap();
    assert_eq!(rows.len(), 1);

    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{"launches":null}}"#).await;
    let api2 = api(&server);
    let rows = api2.execute_select(&select_launches(&api2)).await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn a_scalar_where_rows_were_expected_is_an_error() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{"launches":5}}"#).await;
    let api = api(&server);
    assert!(api.execute_select(&select_launches(&api)).await.is_err());
}

#[tokio::test]
async fn a_response_without_the_root_field_is_an_error() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{"other":[]}}"#).await;
    let api = api(&server);
    let err = api
        .execute_select(&select_launches(&api))
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("root field"), "{err}");
}

#[tokio::test]
async fn a_select_without_a_root_field_sends_nothing() {
    let server = MockServer::start().await;
    let api = api(&server);
    let mut select = api.select();
    select.add_field("id");
    assert!(api.execute_select(&select).await.is_err());
    assert_eq!(requests(&server).await, 0);
}
