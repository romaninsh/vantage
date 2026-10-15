//! `Table<GraphqlApi, _>` against a wiremock endpoint: how a table becomes
//! a query, how the response becomes rows, what a root field that takes no
//! filter does with conditions, and how relations are traversed in Rust.

mod support;

use serde_json::{Value, json};
use support::{mount, requests, test_client};
use vantage_api_client::{
    AnyGraphqlType, FilterDialect, GraphqlApi, GraphqlCondition, GraphqlOp, GraphqlOperation,
    Supports,
};
use vantage_dataset::traits::ReadableValueSet;
use vantage_expressions::{ExprDataSource, Expression};
use vantage_table::column::core::Column;
use vantage_table::sorting::{OrderBy, SortDirection};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::EmptyEntity;
use wiremock::matchers::{body_partial_json, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn builder(server: &MockServer) -> vantage_api_client::GraphqlApiBuilder {
    GraphqlApi::builder(server.uri()).http_client(test_client())
}

fn launches(api: GraphqlApi) -> Table<GraphqlApi, EmptyEntity> {
    Table::new("launches", api)
        .with_id_column("id")
        .with_column_of::<String>("mission_name")
        .with_column_of::<i64>("launch_year")
}

fn col(name: &str) -> Column<AnyGraphqlType> {
    Column::new(name)
}

/// Answer only a request whose body carries `query`.
async fn answer(server: &MockServer, query: &str, root: &str, rows: Value) {
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "query": query })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { (root): rows } })))
        .mount(server)
        .await;
}

async fn ids(table: &Table<GraphqlApi, EmptyEntity>) -> Vec<String> {
    table.list_values().await.unwrap().into_keys().collect()
}

#[tokio::test]
async fn a_table_selects_its_id_then_its_columns_and_keys_rows_by_id() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches { id mission_name launch_year } }",
        "launches",
        json!([
            { "id": "a", "mission_name": "FalconSat", "launch_year": 2006 },
            { "id": 42, "mission_name": "Starlink", "launch_year": 2019 }
        ]),
    )
    .await;
    let rows = launches(builder(&server).build())
        .list_values()
        .await
        .unwrap();

    let keys: Vec<&String> = rows.keys().collect();
    assert_eq!(keys, ["a", "42"]);
    assert_eq!(
        rows["a"].get("mission_name").map(|v| v.value().clone()),
        Some(json!("FalconSat"))
    );
    assert_eq!(
        rows["42"].get("launch_year").map(|v| v.value().clone()),
        Some(json!(2019))
    );
}

#[tokio::test]
async fn a_table_condition_becomes_the_dialects_filter() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches(find: {launch_year: 2018}) { id mission_name launch_year } }",
        "launches",
        json!([{ "id": "1", "mission_name": "Paz", "launch_year": 2018 }]),
    )
    .await;
    let mut table = launches(builder(&server).build());
    table.add_condition(col("launch_year").eq(2018));
    assert_eq!(ids(&table).await, ["1"]);
}

#[tokio::test]
async fn a_hasura_table_orders_with_order_by() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches(order_by: [{mission_name: desc}]) { id mission_name launch_year } }",
        "launches",
        json!([{ "id": "2" }, { "id": "1" }]),
    )
    .await;
    let mut table = launches(builder(&server).dialect(FilterDialect::Hasura).build());
    table.add_order(OrderBy {
        expression: col("mission_name").eq("_"),
        direction: SortDirection::Descending,
    });
    assert_eq!(ids(&table).await, ["2", "1"]);
}

#[tokio::test]
async fn get_some_is_the_first_listed_row() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        200,
        r#"{"data":{"launches":[{"id":"7","mission_name":"x"},{"id":"8","mission_name":"y"}]}}"#,
    )
    .await;
    let table = launches(builder(&server).build());
    let (id, record) = table.get_some_value().await.unwrap().expect("a row");
    assert_eq!(id, "7");
    assert_eq!(
        record.get("mission_name").map(|v| v.value().clone()),
        Some(json!("x"))
    );
}

#[tokio::test]
async fn a_null_root_field_lists_no_rows() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{"launches":null}}"#).await;
    let table = launches(builder(&server).build());
    assert!(table.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_response_that_lacks_the_root_field_is_an_error() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{"other":[]}}"#).await;
    let err = launches(builder(&server).build())
        .list_values()
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("root field"), "{err}");
}

#[tokio::test]
async fn an_object_where_rows_were_expected_is_an_error() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{"launches":{"id":"1"}}}"#).await;
    let err = launches(builder(&server).build())
        .list_values()
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("Unexpected response shape"), "{err}");
}

#[tokio::test]
async fn a_row_the_server_returned_as_a_bare_value_is_an_error() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{"launches":["x"]}}"#).await;
    let err = launches(builder(&server).build())
        .list_values()
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("Expected JSON object"), "{err}");
}

#[tokio::test]
async fn a_row_without_the_id_field_is_an_error_naming_it() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        200,
        r#"{"data":{"launches":[{"mission_name":"x"}]}}"#,
    )
    .await;
    let err = launches(builder(&server).build())
        .list_values()
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("id"), "{err}");
}

#[tokio::test]
async fn a_relay_connection_is_read_through_the_response_path() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches { edges { node { id mission_name launch_year } } } }",
        "launches",
        json!({
            "total": 2,
            "edges": [
                { "node": { "id": "a", "mission_name": "one" } },
                { "node": { "id": "b", "mission_name": "two" } }
            ]
        }),
    )
    .await;
    let table = launches(builder(&server).response_path("edges.node").build());
    assert_eq!(ids(&table).await, ["a", "b"]);
}

#[tokio::test]
async fn root_arguments_are_sent_before_the_filter() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches(input: {}, find: {launch_year: 2018}) { id mission_name launch_year } }",
        "launches",
        json!([{ "id": "1" }]),
    )
    .await;
    let api = builder(&server).root_args(json!({ "input": {} })).build();
    let mut table = launches(api);
    table.add_condition(col("launch_year").eq(2018));
    assert_eq!(ids(&table).await, ["1"]);
}

#[tokio::test]
async fn dotted_columns_are_selected_nested_and_read_flat() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { runs { id stack { id } commit { hash } } }",
        "runs",
        json!([
            { "id": "r1", "stack": { "id": "api-uat" }, "commit": { "hash": "deadbeef" } },
            { "id": "r2", "stack": { "id": "api-dev" }, "commit": null }
        ]),
    )
    .await;
    let table = Table::<GraphqlApi, EmptyEntity>::new("runs", builder(&server).build())
        .with_id_column("id")
        .with_column_of::<String>("stack.id")
        .with_column_of::<String>("commit.hash");
    let rows = table.list_values().await.unwrap();

    let value = |id: &str, key: &str| rows[id].get(key).map(|v| v.value().clone());
    assert_eq!(value("r1", "stack.id"), Some(json!("api-uat")));
    assert_eq!(value("r1", "commit.hash"), Some(json!("deadbeef")));
    assert_eq!(
        value("r2", "commit.hash"),
        Some(Value::Null),
        "a null parent leaves the column present and null"
    );
}

#[tokio::test]
async fn a_dotted_id_column_keys_the_row() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { searchRuns { run { id } } }",
        "searchRuns",
        json!([{ "run": { "id": "r1" } }, { "run": { "id": "r2" } }]),
    )
    .await;
    let table = Table::<GraphqlApi, EmptyEntity>::new("searchRuns", builder(&server).build())
        .with_id_column("run.id");
    assert_eq!(ids(&table).await, ["r1", "r2"]);
}

#[tokio::test]
async fn a_table_counts_by_listing_unless_the_aggregate_is_asked_for() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches { id mission_name launch_year } }",
        "launches",
        json!([{ "id": "1" }, { "id": "2" }, { "id": "3" }]),
    )
    .await;
    let api = builder(&server).build();
    let table = launches(api.clone());
    assert_eq!(api.get_table_count(&table).await.unwrap(), 3);
}

#[tokio::test]
async fn an_aggregate_count_with_a_response_lacking_the_count_is_an_error() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        200,
        r#"{"data":{"launches_aggregate":{"aggregate":null}}}"#,
    )
    .await;
    let api = builder(&server)
        .dialect(FilterDialect::Hasura)
        .supports(Supports::default().with_count(vantage_api_client::CountMode::Aggregate))
        .build();
    let table = launches(api.clone());
    let err = api.get_table_count(&table).await.unwrap_err().to_string();
    assert!(err.contains("aggregate"), "{err}");

    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        200,
        r#"{"data":{"launches_aggregate":{"aggregate":{"count":12}}}}"#,
    )
    .await;
    let api = builder(&server)
        .dialect(FilterDialect::Hasura)
        .supports(Supports::default().with_count(vantage_api_client::CountMode::Aggregate))
        .build();
    let table = launches(api.clone());
    assert_eq!(api.get_table_count(&table).await.unwrap(), 12);
}

#[tokio::test]
async fn an_aggregate_count_checks_the_filter_names_before_sending() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{}}"#).await;
    let api = builder(&server)
        .dialect(FilterDialect::Hasura)
        .supports(Supports::default().with_count(vantage_api_client::CountMode::Aggregate))
        .build();
    let mut table = launches(api.clone());
    table.add_condition(col("a b").eq("x"));
    let err = api.get_table_count(&table).await.unwrap_err().to_string();
    assert!(err.contains("filter key"), "{err}");
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn sums_minimums_and_maximums_are_unsupported() {
    let server = MockServer::start().await;
    let api = builder(&server).build();
    let table = launches(api.clone());
    let year = col("launch_year");
    assert!(
        api.get_table_sum(&table, &year)
            .await
            .unwrap_err()
            .is_unsupported()
    );
    assert!(
        api.get_table_max(&table, &year)
            .await
            .unwrap_err()
            .is_unsupported()
    );
    assert!(
        api.get_table_min(&table, &year)
            .await
            .unwrap_err()
            .is_unsupported()
    );
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn an_expression_cannot_be_executed_and_points_at_the_query_builder() {
    let server = MockServer::start().await;
    let api = builder(&server).build();
    let expr = Expression::<AnyGraphqlType>::new("ignored", vec![]);
    let err = api.execute(&expr).await.unwrap_err().to_string();
    assert!(err.contains("GraphqlSelect"), "{err}");

    let deferred = api.defer(expr);
    let err = deferred.call().await.unwrap_err().to_string();
    assert!(err.contains("GraphqlSelect"), "{err}");
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn eq_conditions_built_from_text_or_values_render_as_equality() {
    let server = MockServer::start().await;
    let api = builder(&server).build();

    let text = <GraphqlApi as TableSource>::eq_condition("name", "Alice").unwrap();
    assert_eq!(
        text.render(FilterDialect::Generic).await.unwrap(),
        json!({ "name": "Alice" })
    );
    let typed = api
        .eq_value_condition("year", AnyGraphqlType::from(2018i64))
        .unwrap();
    assert_eq!(
        typed.render(FilterDialect::Hasura).await.unwrap(),
        json!({ "year": { "_eq": 2018 } })
    );
}

// ── Quicksearch ──────────────────────────────────────────────────────

async fn search_filter(table: &Table<GraphqlApi, EmptyEntity>, text: &str) -> Value {
    let condition = table.data_source().search_table_condition(table, text);
    condition.render(FilterDialect::Hasura).await.unwrap()
}

fn hasura_api() -> GraphqlApi {
    GraphqlApi::builder("https://api.test/graphql")
        .dialect(FilterDialect::Hasura)
        .build()
}

#[tokio::test]
async fn search_is_an_or_of_ilikes_over_the_string_columns_but_not_the_id() {
    let table = Table::<GraphqlApi, EmptyEntity>::new("launches", hasura_api())
        .with_id_column("id")
        .with_column_of::<String>("mission_name")
        .with_column_of::<String>("details")
        .with_column_of::<i64>("launch_year")
        .with_column_of::<String>("rocket.name");
    assert_eq!(
        search_filter(&table, "fal").await,
        json!({ "_or": [
            { "mission_name": { "_ilike": "%fal%" } },
            { "details": { "_ilike": "%fal%" } }
        ] })
    );
}

#[tokio::test]
async fn a_single_searchable_column_is_one_ilike() {
    let table = Table::<GraphqlApi, EmptyEntity>::new("launches", hasura_api())
        .with_id_column("id")
        .with_column_of::<String>("mission_name");
    assert_eq!(
        search_filter(&table, "fal_con").await,
        json!({ "mission_name": { "_ilike": "%fal\\_con%" } })
    );
}

#[tokio::test]
async fn search_escapes_percent_underscore_and_backslash() {
    let table = Table::<GraphqlApi, EmptyEntity>::new("launches", hasura_api())
        .with_id_column("id")
        .with_column_of::<String>("mission_name");
    assert_eq!(
        search_filter(&table, "50%_a\\b").await,
        json!({ "mission_name": { "_ilike": "%50\\%\\_a\\\\b%" } })
    );
}

#[tokio::test]
async fn searchable_flags_take_precedence_over_the_string_columns() {
    use vantage_table::column::flags::ColumnFlag;

    let mut table = Table::<GraphqlApi, EmptyEntity>::new("launches", hasura_api())
        .with_id_column("id")
        .with_column_of::<String>("mission_name");
    table.add_column(col("details").with_flag(ColumnFlag::Searchable));
    assert_eq!(
        search_filter(&table, "fal").await,
        json!({ "details": { "_ilike": "%fal%" } })
    );
}

#[tokio::test]
async fn a_search_with_nothing_to_search_in_matches_nothing() {
    use vantage_table::column::flags::ColumnFlag;

    let numbers = Table::<GraphqlApi, EmptyEntity>::new("launches", hasura_api())
        .with_id_column("id")
        .with_column_of::<i64>("launch_year");
    assert_eq!(search_filter(&numbers, "fal").await, json!({ "_not": {} }));

    let mut dotted_only = Table::<GraphqlApi, EmptyEntity>::new("launches", hasura_api())
        .with_id_column("id")
        .with_column_of::<String>("mission_name");
    dotted_only.add_column(col("rocket.name").with_flag(ColumnFlag::Searchable));
    assert_eq!(
        search_filter(&dotted_only, "fal").await,
        json!({ "_not": {} })
    );
}

// ── Conditions applied locally ───────────────────────────────────────

const LOCAL_ROWS: &str = r#"{"data":{"launches":[
    {"id":"1","name":"alpha","year":2006,"stack":{"id":"api-uat"},"note":null},
    {"id":"2","name":"beta","year":2015,"stack":{"id":"api-dev"},"note":"x"},
    {"id":"3","name":"gamma_50%","year":2020,"stack":{"id":"web-uat"},"note":"y"}
]}}"#;

/// A `launches` table over a root field that takes no filter argument.
async fn unfiltered(server: &MockServer) -> Table<GraphqlApi, EmptyEntity> {
    mount(server, "POST", 200, LOCAL_ROWS).await;
    let api = builder(server)
        .supports(Supports::default().with_filter(false))
        .build();
    Table::new("launches", api)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("year")
        .with_column_of::<String>("stack.id")
        .with_column_of::<String>("note")
}

async fn local_ids(condition: GraphqlCondition) -> Vec<String> {
    let server = MockServer::start().await;
    let mut table = unfiltered(&server).await;
    table.add_condition(condition);
    ids(&table).await
}

#[tokio::test]
async fn without_filter_push_down_the_query_carries_no_filter_and_the_rows_narrow_locally() {
    let server = MockServer::start().await;
    let mut table = unfiltered(&server).await;
    table.add_condition(col("stack.id").eq("api-uat"));
    assert_eq!(ids(&table).await, ["1"]);

    let received = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&received[0].body).unwrap();
    assert_eq!(
        body["query"],
        json!("query { launches { id name year stack { id } note } }")
    );
}

#[tokio::test]
async fn local_comparisons_follow_the_operators() {
    assert_eq!(local_ids(col("year").gt(2010)).await, ["2", "3"]);
    assert_eq!(local_ids(col("year").gte(2015)).await, ["2", "3"]);
    assert_eq!(local_ids(col("year").lt(2015)).await, ["1"]);
    assert_eq!(local_ids(col("year").lte(2015)).await, ["1", "2"]);
    assert_eq!(local_ids(col("year").ne(2015)).await, ["1", "3"]);
    assert_eq!(local_ids(col("name").gt("alpha")).await, ["2", "3"]);
    assert_eq!(local_ids(col("name").lte("beta")).await, ["1", "2"]);
    assert_eq!(local_ids(col("name").eq("beta")).await, ["2"]);
}

#[tokio::test]
async fn local_set_membership_and_nulls() {
    assert_eq!(
        local_ids(col("year").in_(vec![2006i64, 2020])).await,
        ["1", "3"]
    );
    assert_eq!(
        local_ids(col("year").not_in(vec![2006i64, 2020])).await,
        ["2"]
    );
    assert_eq!(local_ids(col("note").is_null()).await, ["1"]);
    assert_eq!(local_ids(col("note").is_not_null()).await, ["2", "3"]);
    assert_eq!(
        local_ids(col("missing").is_null()).await,
        ["1", "2", "3"],
        "an absent field reads as null"
    );
}

#[tokio::test]
async fn local_like_honours_the_anchors() {
    assert_eq!(local_ids(col("name").like("al%")).await, ["1"], "prefix");
    assert_eq!(local_ids(col("name").like("%ta")).await, ["2"], "suffix");
    assert_eq!(
        local_ids(col("name").like("%mm%")).await,
        ["3"],
        "substring"
    );
    assert_eq!(local_ids(col("name").like("beta")).await, ["2"], "exact");
    assert!(local_ids(col("name").like("BETA")).await.is_empty());
    assert_eq!(local_ids(col("name").ilike("BETA")).await, ["2"]);
    assert_eq!(local_ids(col("name").ilike("%A")).await, ["1", "2"]);
    assert!(
        local_ids(col("year").like("20%")).await.is_empty(),
        "numbers are not text"
    );
}

#[tokio::test]
async fn a_backslash_escaped_wildcard_matches_itself() {
    assert_eq!(local_ids(col("name").like("%50\\%")).await, ["3"]);
    assert!(local_ids(col("name").like("%5\\%")).await.is_empty());
    assert_eq!(local_ids(col("name").like("%ma\\_50%")).await, ["3"]);
}

#[tokio::test]
async fn local_groups_and_or_not() {
    let both = GraphqlCondition::And(vec![col("year").gt(2010), col("note").eq("y")]);
    assert_eq!(local_ids(both).await, ["3"]);

    let either = GraphqlCondition::Or(vec![col("year").lt(2010), col("note").eq("y")]);
    assert_eq!(local_ids(either).await, ["1", "3"]);

    let neither = GraphqlCondition::Not(Box::new(either_of_years()));
    assert_eq!(local_ids(neither).await, ["2"]);

    let nothing = GraphqlCondition::Not(Box::new(GraphqlCondition::And(Vec::new())));
    assert!(local_ids(nothing).await.is_empty());
}

fn either_of_years() -> GraphqlCondition {
    GraphqlCondition::Or(vec![col("year").eq(2006), col("year").eq(2020)])
}

#[tokio::test]
async fn a_deferred_value_resolves_once_for_all_rows() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use vantage_expressions::{DeferredFn, ExpressiveEnum};

    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let condition = GraphqlCondition::deferred_field(
        "stack.id",
        GraphqlOp::Eq,
        DeferredFn::new(move || {
            let counter = Arc::clone(&counter);
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(ExpressiveEnum::Scalar(AnyGraphqlType::new(
                    "api-uat".to_string(),
                )))
            })
        }),
    );
    assert_eq!(local_ids(condition).await, ["1"]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn deferred_conditions_inside_groups_are_resolved_too() {
    use vantage_expressions::{DeferredFn, ExpressiveEnum};

    let deferred = |value: &'static str| {
        GraphqlCondition::deferred_field(
            "stack.id",
            GraphqlOp::Eq,
            DeferredFn::new(move || {
                Box::pin(async move {
                    Ok(ExpressiveEnum::Scalar(AnyGraphqlType::new(
                        value.to_string(),
                    )))
                })
            }),
        )
    };
    let either = GraphqlCondition::Or(vec![deferred("api-uat"), deferred("web-uat")]);
    assert_eq!(local_ids(either).await, ["1", "3"]);

    let not_uat = GraphqlCondition::Not(Box::new(deferred("api-uat")));
    assert_eq!(local_ids(not_uat).await, ["2", "3"]);

    let and = GraphqlCondition::And(vec![deferred("api-uat"), col("year").lt(2010)]);
    assert_eq!(local_ids(and).await, ["1"]);
}

#[tokio::test]
async fn a_filter_that_cannot_be_evaluated_row_by_row_is_refused() {
    use vantage_expressions::{DeferredFn, ExpressiveEnum};

    let opaque = GraphqlCondition::Deferred(DeferredFn::new(|| {
        Box::pin(async {
            Ok(ExpressiveEnum::Scalar(AnyGraphqlType::untyped(
                json!({ "id": { "_eq": "1" } }),
            )))
        })
    }));
    let server = MockServer::start().await;
    let mut table = unfiltered(&server).await;
    table.add_condition(opaque);
    assert!(table.list_values().await.is_err());
}

// ── Traversal in Rust ────────────────────────────────────────────────

fn rockets(api: GraphqlApi) -> Table<GraphqlApi, EmptyEntity> {
    Table::<GraphqlApi, EmptyEntity>::new("rockets", api)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_many("launches", "rocket_id", |api| {
            Table::<GraphqlApi, EmptyEntity>::new("launches", api)
                .with_id_column("id")
                .with_column_of::<String>("rocket_id")
        })
}

fn launches_with_rocket(api: GraphqlApi) -> Table<GraphqlApi, EmptyEntity> {
    Table::<GraphqlApi, EmptyEntity>::new("launches", api)
        .with_id_column("id")
        .with_column_of::<String>("rocket_id")
        .with_one("rocket", "rocket_id", |api| {
            Table::<GraphqlApi, EmptyEntity>::new("rockets", api)
                .with_id_column("id")
                .with_column_of::<String>("name")
        })
}

#[tokio::test]
async fn a_parent_narrowed_by_id_gives_the_child_a_concrete_filter() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches(find: {rocket_id: \"falcon9\"}) { id rocket_id } }",
        "launches",
        json!([{ "id": "l1", "rocket_id": "falcon9" }]),
    )
    .await;
    let mut parent = rockets(builder(&server).build());
    parent.add_condition(col("id").eq("falcon9"));

    let child = parent.get_ref_as::<EmptyEntity>("launches").unwrap();
    assert_eq!(ids(&child).await, ["l1"]);
    assert_eq!(requests(&server).await, 1, "no round trip for the parent");
}

#[tokio::test]
async fn the_child_filter_follows_the_dialect() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches(where: {rocket_id: {_eq: \"falcon9\"}}) { id rocket_id } }",
        "launches",
        json!([{ "id": "l1", "rocket_id": "falcon9" }]),
    )
    .await;
    let mut parent = rockets(builder(&server).dialect(FilterDialect::Hasura).build());
    parent.add_condition(col("id").eq("falcon9"));
    let child = parent.get_ref_as::<EmptyEntity>("launches").unwrap();
    assert_eq!(ids(&child).await, ["l1"]);
}

#[tokio::test]
async fn a_foreign_key_held_in_the_parent_row_is_read_at_request_time() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches { id rocket_id } }",
        "launches",
        json!([{ "id": "l1", "rocket_id": "falcon9" }, { "id": "l2", "rocket_id": "other" }]),
    )
    .await;
    answer(
        &server,
        "query { rockets(find: {id: \"falcon9\"}) { id name } }",
        "rockets",
        json!([{ "id": "falcon9", "name": "Falcon 9" }]),
    )
    .await;
    let parent = launches_with_rocket(builder(&server).build());

    let child = parent.get_ref_as::<EmptyEntity>("rocket").unwrap();
    assert_eq!(
        requests(&server).await,
        0,
        "nothing is sent until the child is read"
    );
    assert_eq!(ids(&child).await, ["falcon9"]);
    assert_eq!(
        requests(&server).await,
        2,
        "one round trip for the parent, one for the child"
    );
}

#[tokio::test]
async fn a_parent_with_no_rows_cannot_supply_the_foreign_key() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{"launches":[]}}"#).await;
    let parent = launches_with_rocket(builder(&server).build());
    let child = parent.get_ref_as::<EmptyEntity>("rocket").unwrap();
    let err = child.list_values().await.unwrap_err().to_string();
    assert!(err.contains("parent"), "{err}");
}

#[tokio::test]
async fn get_by_id_reads_a_single_object_under_the_root_field() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { launches(find: {id: \"a\"}) { id mission_name launch_year } }",
        "launches",
        json!({ "id": "a", "mission_name": "FalconSat" }),
    )
    .await;
    let api = builder(&server).build();
    let table = launches(api.clone());
    let record = api
        .get_table_value(&table, &"a".to_string())
        .await
        .unwrap()
        .expect("the row");
    assert_eq!(
        record.get("mission_name").map(|v| v.value().clone()),
        Some(json!("FalconSat"))
    );
}

#[tokio::test]
async fn get_by_id_is_none_when_the_root_field_is_null() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{"launches":null}}"#).await;
    let api = builder(&server).build();
    let table = launches(api.clone());
    assert!(
        api.get_table_value(&table, &"zzz".to_string())
            .await
            .unwrap()
            .is_none()
    );
}
