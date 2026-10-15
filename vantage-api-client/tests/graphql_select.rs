//! `GraphqlSelect`: the query document and variables it renders, and the
//! synchronous preview of the same document. Filters render inline as
//! GraphQL value syntax; paging goes through typed `$limit` / `$offset`
//! variables.

use serde_json::json;
use vantage_api_client::{FilterDialect, GraphqlCondition, GraphqlSelect};
use vantage_expressions::{DeferredFn, ExpressiveEnum, Order};

fn launches() -> GraphqlSelect {
    GraphqlSelect::new()
        .with_root_field("launches")
        .with_field("id")
}

#[tokio::test]
async fn a_minimal_select_is_one_root_field_with_its_fields() {
    let rendered = GraphqlSelect::new()
        .with_root_field("launches")
        .with_field("id")
        .with_field("mission_name")
        .render()
        .await
        .unwrap();
    assert_eq!(rendered.query, "query { launches { id mission_name } }");
    assert!(rendered.variables.is_empty());
}

#[tokio::test]
async fn the_generic_dialect_filters_with_find() {
    let rendered = launches()
        .with_dialect(FilterDialect::Generic)
        .with_condition(GraphqlCondition::eq("mission_name", "FalconSat"))
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query { launches(find: {mission_name: \"FalconSat\"}) { id } }"
    );
    assert!(rendered.variables.is_empty());
}

#[tokio::test]
async fn the_hasura_dialect_filters_with_where() {
    let rendered = launches()
        .with_dialect(FilterDialect::Hasura)
        .with_condition(GraphqlCondition::eq("year", 2018))
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query { launches(where: {year: {_eq: 2018}}) { id } }"
    );
}

#[tokio::test]
async fn a_filter_argument_name_replaces_the_dialects() {
    let rendered = launches()
        .with_filter_arg_name("filter")
        .with_condition(GraphqlCondition::eq("year", 2018))
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query { launches(filter: {year: 2018}) { id } }"
    );
}

#[tokio::test]
async fn several_conditions_are_anded_into_one_filter() {
    let rendered = launches()
        .with_condition(GraphqlCondition::eq("year", 2018))
        .with_condition(GraphqlCondition::eq("site", "KSC"))
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query { launches(find: {year: 2018, site: \"KSC\"}) { id } }"
    );
}

#[tokio::test]
async fn filter_values_render_as_graphql_values() {
    let rendered = launches()
        .with_dialect(FilterDialect::Hasura)
        .with_condition(GraphqlCondition::eq("flag", true))
        .with_condition(GraphqlCondition::eq("gone", json!(null)))
        .with_condition(GraphqlCondition::eq("tags", json!(["a", "b"])))
        .with_condition(GraphqlCondition::eq("pos", json!({ "x": 1.5 })))
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query { launches(where: {flag: {_eq: true}, gone: {_eq: null}, tags: {_eq: [\"a\", \"b\"]}, pos: {_eq: {x: 1.5}}}) { id } }"
    );
}

#[tokio::test]
async fn string_values_are_escaped() {
    let rendered = launches()
        .with_condition(GraphqlCondition::eq(
            "note",
            "say \"hi\" \\ now\nnext\tcol\r",
        ))
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query { launches(find: {note: \"say \\\"hi\\\" \\\\ now\\nnext\\tcol\\r\"}) { id } }"
    );
}

#[tokio::test]
async fn control_characters_never_reach_the_document_raw() {
    let text = "a\u{0}b\u{8}c\u{c}d\u{1b}[31m\u{1f}\u{7f}";
    let rendered = launches()
        .with_condition(GraphqlCondition::eq("note", text))
        .render()
        .await
        .unwrap();
    assert!(
        !rendered.query.chars().any(|c| c.is_ascii_control()),
        "{:?}",
        rendered.query
    );
    assert!(rendered.query.contains("\\u0000"), "{}", rendered.query);
    assert!(rendered.query.contains("\\u001B"), "{}", rendered.query);
    assert!(rendered.query.contains("\\u007F"), "{}", rendered.query);
}

#[tokio::test]
async fn text_beyond_ascii_passes_through() {
    let rendered = launches()
        .with_condition(GraphqlCondition::eq("city", "Zürich ✓"))
        .render()
        .await
        .unwrap();
    assert!(
        rendered.query.contains("\"Zürich ✓\""),
        "{}",
        rendered.query
    );
}

#[tokio::test]
async fn paging_goes_through_variables() {
    let rendered = launches()
        .with_limit(Some(10), Some(20))
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query ($limit: Int, $offset: Int) { launches(limit: $limit, offset: $offset) { id } }"
    );
    assert_eq!(rendered.variables.get("limit"), Some(&json!(10)));
    assert_eq!(rendered.variables.get("offset"), Some(&json!(20)));
}

#[tokio::test]
async fn a_limit_alone_declares_only_the_limit() {
    let rendered = launches().with_limit(Some(5), None).render().await.unwrap();
    assert_eq!(
        rendered.query,
        "query ($limit: Int) { launches(limit: $limit) { id } }"
    );
    assert_eq!(rendered.variables.len(), 1);
}

#[tokio::test]
async fn the_filter_comes_before_the_paging_arguments() {
    let rendered = launches()
        .with_condition(GraphqlCondition::eq("year", 2018))
        .with_limit(Some(10), Some(0))
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query ($limit: Int, $offset: Int) { launches(find: {year: 2018}, limit: $limit, offset: $offset) { id } }"
    );
}

#[tokio::test]
async fn an_operation_name_follows_the_query_keyword() {
    let named = launches().with_operation_name("GetLaunches");
    assert_eq!(
        named.render().await.unwrap().query,
        "query GetLaunches { launches { id } }"
    );
    let paged = named.with_limit(Some(5), None);
    assert_eq!(
        paged.render().await.unwrap().query,
        "query GetLaunches($limit: Int) { launches(limit: $limit) { id } }"
    );
}

#[tokio::test]
async fn hasura_sorts_with_order_by_and_generic_leaves_the_sort_out() {
    let hasura = GraphqlSelect::new()
        .with_root_field("users")
        .with_field("id")
        .with_dialect(FilterDialect::Hasura)
        .with_order("created_at", Order::Desc)
        .with_order("name", Order::Asc);
    assert_eq!(
        hasura.render().await.unwrap().query,
        "query { users(order_by: [{created_at: desc, name: asc}]) { id } }"
    );

    let generic = GraphqlSelect::new()
        .with_root_field("users")
        .with_field("id")
        .with_order("created_at", Order::Desc);
    assert_eq!(
        generic.render().await.unwrap().query,
        "query { users { id } }"
    );
}

#[tokio::test]
async fn a_sub_selection_carries_its_own_arguments() {
    let rocket = GraphqlSelect::new().with_field("id").with_field("name");
    let rendered = launches()
        .with_field("mission_name")
        .with_sub_selection("rocket", rocket)
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query { launches { id mission_name rocket { id name } } }"
    );

    let filtered = GraphqlSelect::new()
        .with_field("id")
        .with_condition(GraphqlCondition::eq("kind", "heavy"))
        .with_limit(Some(2), Some(1));
    let rendered = launches()
        .with_sub_selection("rockets", filtered)
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query { launches { id rockets(find: {kind: \"heavy\"}, limit: 2, offset: 1) { id } } }"
    );
    assert!(rendered.variables.is_empty());
}

#[tokio::test]
async fn dotted_fields_group_into_nested_selections() {
    let rendered = GraphqlSelect::new()
        .with_root_field("searchRuns")
        .with_field("run.id")
        .with_field("run.state")
        .with_field("run.commit.hash")
        .with_field("stack.id")
        .with_field("isModule")
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query { searchRuns { run { id state commit { hash } } stack { id } isModule } }"
    );
}

#[tokio::test]
async fn root_arguments_render_before_the_filter() {
    let rendered = GraphqlSelect::new()
        .with_root_field("searchRuns")
        .with_root_args(json!({ "input": {} }))
        .with_field("run.id")
        .with_condition(GraphqlCondition::eq("state", "OK"))
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query { searchRuns(input: {}, find: {state: \"OK\"}) { run { id } } }"
    );
}

#[tokio::test]
async fn root_arguments_that_are_not_an_object_are_ignored() {
    let rendered = launches()
        .with_root_args(json!("input"))
        .render()
        .await
        .unwrap();
    assert_eq!(rendered.query, "query { launches { id } }");
}

#[tokio::test]
async fn the_response_path_wraps_the_selection_set() {
    let rendered = GraphqlSelect::new()
        .with_root_field("searchRuns")
        .with_root_args(json!({ "input": {} }))
        .with_response_path(vec!["edges".into(), "node".into()])
        .with_field("run.id")
        .with_field("stack.id")
        .render()
        .await
        .unwrap();
    assert_eq!(
        rendered.query,
        "query { searchRuns(input: {}) { edges { node { run { id } stack { id } } } } }"
    );
}

#[tokio::test]
async fn an_empty_selection_set_is_an_error() {
    let err = GraphqlSelect::new()
        .with_root_field("launches")
        .render()
        .await
        .unwrap_err();
    assert!(err.to_string().contains("selection set"), "{err}");
}

#[tokio::test]
async fn a_missing_root_field_is_an_error() {
    let err = GraphqlSelect::new()
        .with_field("id")
        .render()
        .await
        .unwrap_err();
    assert!(err.to_string().contains("root_field"), "{err}");
}

#[tokio::test]
async fn a_dialect_that_cannot_spell_the_filter_fails_the_render() {
    let err = launches()
        .with_condition(GraphqlCondition::Or(vec![
            GraphqlCondition::eq("a", 1),
            GraphqlCondition::eq("b", 2),
        ]))
        .render()
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Generic"), "{err}");
}

#[tokio::test]
async fn every_name_spliced_into_the_document_is_checked() {
    let cases = [
        (
            launches().with_root_field("launches { secret }"),
            "root field",
        ),
        (launches().with_field("run.id }"), "field"),
        (launches().with_operation_name("Q { x }"), "operation name"),
        (
            launches().with_response_path(vec!["edges".into(), "no de".into()]),
            "response path segment",
        ),
        (
            launches().with_order("id: asc}, {x", Order::Asc),
            "order field",
        ),
        (
            launches()
                .with_filter_arg_name("find)")
                .with_condition(GraphqlCondition::eq("id", "1")),
            "filter argument",
        ),
        (
            launches().with_root_args(json!({ "in put": {} })),
            "root argument",
        ),
        (
            launches().with_root_args(json!({ "input": { "a}": 1 } })),
            "root argument key",
        ),
        (
            launches().with_sub_selection("rocket x", GraphqlSelect::new().with_field("id")),
            "field",
        ),
        (
            launches().with_condition(GraphqlCondition::eq(
                "name}) { id } q: launches(find: {x",
                "v",
            )),
            "filter key",
        ),
    ];
    for (select, kind) in cases {
        let err = select.render().await.unwrap_err().to_string();
        assert!(err.contains("GraphQL name is not valid"), "{kind}: {err}");
        assert!(err.contains(kind), "{kind}: {err}");
    }
}

#[tokio::test]
async fn object_keys_inside_filter_values_are_checked_at_every_depth() {
    let fine = json!({ "_eq": [{ "b": 1 }] });
    let rendered = launches()
        .with_condition(GraphqlCondition::eq("a", fine))
        .render()
        .await;
    assert!(rendered.is_ok());

    for bad in [
        json!({ "b c": 1 }),
        json!([{ "b c": 1 }]),
        json!({ "ok": { "x}": 1 } }),
    ] {
        let err = launches()
            .with_condition(GraphqlCondition::eq("a", bad))
            .render()
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("filter key"), "{err}");
    }

    let text_is_not_a_name = launches()
        .with_condition(GraphqlCondition::eq("a", json!({ "k": "b c }" })))
        .render()
        .await;
    assert!(text_is_not_a_name.is_ok(), "values are not names");
}

#[tokio::test]
async fn names_follow_the_graphql_grammar() {
    for ok in ["id", "_id", "__typename", "a1", "mission_name", "X"] {
        let rendered = launches().with_field(ok).render().await;
        assert!(rendered.is_ok(), "{ok}");
    }
    for bad in [
        "1a", "a-b", "a b", "a}", "é", "a\n", "run.", ".run", "run..id", "run.1d",
    ] {
        let err = launches().with_field(bad).render().await.unwrap_err();
        assert!(
            err.to_string().contains("GraphQL name is not valid"),
            "{bad:?}: {err}"
        );
    }
}

#[test]
fn the_accessors_report_what_was_set() {
    let select = launches()
        .with_dialect(FilterDialect::Hasura)
        .with_condition(GraphqlCondition::eq("a", 1));
    assert_eq!(select.root_field(), Some("launches"));
    assert_eq!(select.fields(), ["id"]);
    assert_eq!(select.conditions().len(), 1);
    assert_eq!(select.dialect(), FilterDialect::Hasura);
    assert_eq!(GraphqlSelect::new().root_field(), None);
    assert_eq!(GraphqlSelect::new().dialect(), FilterDialect::Generic);
}

#[tokio::test]
async fn the_preview_is_the_rendered_document_for_a_literal_query() {
    let select = GraphqlSelect::new()
        .with_root_field("launches")
        .with_field("id")
        .with_field("rocket.name")
        .with_dialect(FilterDialect::Generic)
        .with_condition(GraphqlCondition::eq("mission_name", "FalconSat"));
    let rendered = select.render().await.unwrap();
    assert_eq!(select.preview(), rendered.query);
    assert!(select.preview().contains("\"FalconSat\""));
}

#[tokio::test]
async fn the_preview_matches_render_for_a_rich_query() {
    let select = GraphqlSelect::new()
        .with_root_field("searchRuns")
        .with_operation_name("Runs")
        .with_root_args(json!({ "input": {} }))
        .with_response_path(vec!["edges".into(), "node".into()])
        .with_dialect(FilterDialect::Hasura)
        .with_field("run.id")
        .with_order("state", Order::Asc)
        .with_condition(GraphqlCondition::eq("state", "OK"))
        .with_sub_selection(
            "stack",
            GraphqlSelect::new()
                .with_field("id")
                .with_condition(GraphqlCondition::eq("kind", "x")),
        );
    let rendered = select.render().await.unwrap();
    assert_eq!(select.preview(), rendered.query);
}

#[test]
fn the_preview_inlines_paging() {
    let preview = launches().with_limit(Some(5), Some(10)).preview();
    assert_eq!(preview, "query { launches(limit: 5, offset: 10) { id } }");

    let named = launches().with_operation_name("Q").preview();
    assert_eq!(named, "query Q { launches { id } }");

    let child = GraphqlSelect::new()
        .with_field("id")
        .with_condition(GraphqlCondition::eq("kind", "x"))
        .with_limit(Some(2), Some(1));
    let nested = launches().with_sub_selection("rockets", child).preview();
    assert_eq!(
        nested,
        "query { launches { id rockets(find: {kind: \"x\"}, limit: 2, offset: 1) { id } } }"
    );
}

#[test]
fn the_preview_marks_a_deferred_condition_instead_of_awaiting_it() {
    let deferred = GraphqlCondition::deferred_field(
        "stack_id",
        vantage_api_client::GraphqlOp::Eq,
        DeferredFn::new(|| {
            Box::pin(async {
                Ok(ExpressiveEnum::Scalar(
                    vantage_api_client::AnyGraphqlType::from("x"),
                ))
            })
        }),
    );
    let preview = launches().with_condition(deferred.clone()).preview();
    assert!(preview.contains("**deferred(stack_id"), "{preview}");

    let child = GraphqlSelect::new()
        .with_field("id")
        .with_condition(deferred);
    let nested = launches().with_sub_selection("stacks", child).preview();
    assert!(nested.contains("**deferred(stack_id"), "{nested}");
}

#[test]
fn the_preview_shows_what_the_dialect_cannot_spell() {
    let preview = launches()
        .with_condition(GraphqlCondition::Or(vec![GraphqlCondition::eq("a", 1)]))
        .preview();
    assert!(preview.contains("<unrenderable"), "{preview}");

    let child = GraphqlSelect::new()
        .with_field("id")
        .with_condition(GraphqlCondition::Not(Box::new(GraphqlCondition::eq(
            "a", 1,
        ))));
    let nested = launches().with_sub_selection("rockets", child).preview();
    assert!(nested.contains("<unrenderable"), "{nested}");
}

#[test]
fn the_preview_reports_what_is_missing() {
    let no_fields = GraphqlSelect::new().with_root_field("launches").preview();
    assert!(no_fields.contains("<empty selection set>"), "{no_fields}");

    let no_root = GraphqlSelect::new().with_field("id").preview();
    assert!(no_root.contains("<no root_field>"), "{no_root}");
}

#[test]
fn the_preview_orders_hasura_selects_only() {
    let hasura = launches()
        .with_dialect(FilterDialect::Hasura)
        .with_order("id", Order::Desc)
        .preview();
    assert_eq!(hasura, "query { launches(order_by: [{id: desc}]) { id } }");
    let generic = launches().with_order("id", Order::Desc).preview();
    assert_eq!(generic, "query { launches { id } }");
}
