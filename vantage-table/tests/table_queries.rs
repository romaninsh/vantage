#![cfg(feature = "mocks")]

//! What narrows and shapes a table's reads: conditions, search, ordering,
//! pagination, and the select and counts built from them.

use serde_json::{Value, json};
use vantage_dataset::prelude::*;
use vantage_expressions::mocks::datasource::MockSelectableDataSource;
use vantage_expressions::mocks::mock_builder;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::selectable::Selectable;
use vantage_expressions::{Expression, ExpressiveEnum, expr, expr_any};
use vantage_table::conditions::literal_equality;
use vantage_table::mocks::MockColumn;
use vantage_table::prelude::*;
use vantage_table::table::{Hook, SelectCx, select_hook};
use vantage_types::{EmptyEntity, Record};

type MockTable = Table<MockTableSource, EmptyEntity>;

fn table() -> MockTable {
    MockTable::new("test", MockTableSource::new())
}

fn previews(table: &MockTable) -> Vec<String> {
    table.conditions().map(|c| c.preview()).collect()
}

// --- conditions ---------------------------------------------------------------

#[test]
fn conditions_are_listed_in_the_order_they_were_added() {
    let mut table = table();
    table.add_condition(expr_any!("perm1"));
    let handle = table.temp_add_condition(expr_any!("temp1"));
    table.add_condition(expr_any!("perm2"));
    assert_eq!(previews(&table), ["perm1", "temp1", "perm2"]);

    table.temp_remove_condition(handle).unwrap();
    assert_eq!(previews(&table), ["perm1", "perm2"]);
}

#[test]
fn with_condition_is_the_builder_form_of_add_condition() {
    let table = table()
        .with_condition(expr_any!("a"))
        .with_condition(expr_any!("b"));
    assert_eq!(previews(&table), ["a", "b"]);
}

#[test]
fn removing_a_temporary_condition_twice_is_not_found() {
    let mut table = table();
    let handle = table.temp_add_condition(expr_any!("temp"));
    table.temp_remove_condition(handle).unwrap();

    let err = table.temp_remove_condition(handle).unwrap_err();
    assert!(err.is_not_found(), "{err}");
    assert_eq!(table.conditions().count(), 0);
}

#[test]
fn replace_temp_condition_adds_swaps_and_clears() {
    let mut table = table();
    table.add_condition(expr_any!("perm"));
    let first = table
        .replace_temp_condition(None, Some(expr_any!("a")))
        .unwrap();
    assert!(first.is_some());
    let second = table
        .replace_temp_condition(first, Some(expr_any!("b")))
        .unwrap();
    assert_eq!(previews(&table), ["perm", "b"]);

    let cleared = table.replace_temp_condition(second, None).unwrap();
    assert!(cleared.is_none());
    assert_eq!(previews(&table), ["perm"]);
}

#[test]
fn replace_temp_condition_with_a_stale_handle_fails_before_adding_the_new_one() {
    let mut table = table();
    let first = table
        .replace_temp_condition(None, Some(expr_any!("a")))
        .unwrap();
    table.replace_temp_condition(first, None).unwrap();

    let err = table
        .replace_temp_condition(first, Some(expr_any!("c")))
        .unwrap_err();
    assert!(err.is_not_found(), "{err}");
    assert_eq!(table.conditions().count(), 0, "a refused swap adds nothing");
}

#[test]
fn replace_temp_condition_with_nothing_to_remove_or_add_changes_nothing() {
    let mut table = table();
    assert!(table.replace_temp_condition(None, None).unwrap().is_none());
    assert_eq!(table.conditions().count(), 0);
}

fn eq_expr(lhs: &str, v: Value) -> Expression<Value> {
    Expression::new(
        "{} = {}",
        vec![
            ExpressiveEnum::Nested(Expression::new(lhs, vec![])),
            ExpressiveEnum::Nested(Expression::new("{}", vec![ExpressiveEnum::Scalar(v)])),
        ],
    )
}

#[test]
fn a_literal_equality_condition_registers_an_invariant() {
    let mut table = table();
    table.add_condition(eq_expr("parent", json!("p1")));
    assert_eq!(table.invariants().get("parent"), Some(&json!("p1")));
    assert_eq!(table.conditions().count(), 1);
}

#[test]
fn an_expression_condition_registers_no_invariant() {
    let mut table = table();
    table.add_condition(expr_any!("price > {}", 10));
    assert!(table.invariants().is_empty());
}

#[test]
fn literal_equality_reads_a_bare_or_quoted_column_and_its_value() {
    for lhs in ["parent", "\"parent\"", "`parent`"] {
        assert_eq!(
            literal_equality(&eq_expr(lhs, json!("p1"))),
            Some(("parent".to_string(), json!("p1"))),
            "{lhs}"
        );
    }
}

#[test]
fn literal_equality_rejects_a_table_qualified_column() {
    assert_eq!(
        literal_equality(&eq_expr("\"t\".\"parent\"", json!(1))),
        None
    );
}

#[test]
fn literal_equality_rejects_empty_and_non_identifier_columns() {
    for lhs in ["\"\"", "``", "two words", "1abc", ""] {
        assert_eq!(literal_equality(&eq_expr(lhs, json!(1))), None, "{lhs:?}");
    }
}

#[test]
fn literal_equality_rejects_any_operator_other_than_equals() {
    let gt = Expression::new("{} > {}", eq_expr("price", json!(1)).parameters);
    assert_eq!(literal_equality(&gt), None);
}

fn op_expr(template: &str, lhs: &str, v: Value) -> Expression<Value> {
    Expression::new(template, eq_expr(lhs, v).parameters)
}

#[test]
fn describe_condition_reads_as_the_expression() {
    let t = table();
    assert_eq!(
        t.data_source()
            .describe_condition(&eq_expr("parent", json!("p1"))),
        "parent = \"p1\""
    );
}

#[test]
fn condition_column_names_eq_in_and_comparisons() {
    let t = table();
    let src = t.data_source();
    for template in [
        "{} = {}",
        "{} > {}",
        "{} <= {}",
        "{} IN ({})",
        "{} in {}",
        "{} LIKE {}",
    ] {
        assert_eq!(
            src.condition_column(&op_expr(template, "invoice_id", json!(1))),
            Some("invoice_id".to_string()),
            "{template}"
        );
    }
    assert_eq!(
        src.condition_column(&op_expr("{} = {}", "\"invoice_id\"", json!(1))),
        Some("invoice_id".to_string())
    );
}

#[test]
fn condition_column_is_none_for_raw_expressions() {
    let t = table();
    let src = t.data_source();
    assert_eq!(src.condition_column(&expr_any!("1 = 1")), None);
    assert_eq!(src.condition_column(&expr_any!("price > {}", 10)), None);
    assert_eq!(
        src.condition_column(&op_expr("{} = {}", "\"t\".\"col\"", json!(1))),
        None
    );
    assert_eq!(
        src.condition_column(&op_expr("{} AND {}", "a", json!(1))),
        None
    );
    assert_eq!(
        src.condition_column(&op_expr("{} IN ({}", "a", json!(1))),
        None
    );
}

#[test]
fn condition_column_is_none_for_negations() {
    let t = table();
    let src = t.data_source();
    for template in ["{} != {}", "{} <> {}", "{} NOT IN ({})", "{} NOT LIKE {}"] {
        assert_eq!(
            src.condition_column(&op_expr(template, "tenant_id", json!(1))),
            None,
            "{template}"
        );
    }
}

fn column_vs(lhs: &str, rhs: Expression<Value>) -> Expression<Value> {
    Expression::new(
        "{} = {}",
        vec![
            ExpressiveEnum::Nested(Expression::new(lhs, vec![])),
            ExpressiveEnum::Nested(rhs),
        ],
    )
}

#[test]
fn condition_column_is_none_for_column_vs_column() {
    let t = table();
    let src = t.data_source();
    let same = column_vs("tenant_id", Expression::new("tenant_id", vec![]));
    assert_eq!(src.condition_column(&same), None);
}

#[test]
fn condition_column_is_none_for_an_or_combined_operand() {
    let t = table();
    let src = t.data_source();
    let or = column_vs("tenant_id", expr_any!("7 OR 1 = 1"));
    assert_eq!(src.condition_column(&or), None);
}

#[test]
fn condition_column_names_an_in_over_a_subquery() {
    let t = table();
    let src = t.data_source();
    let sub = Expression::new(
        "{} IN ({})",
        vec![
            ExpressiveEnum::Nested(Expression::new("tenant_id", vec![])),
            ExpressiveEnum::Nested(expr_any!("SELECT id FROM tenant WHERE name = {}", "acme")),
        ],
    );
    assert_eq!(src.condition_column(&sub), Some("tenant_id".to_string()));
}

#[test]
fn literal_equality_rejects_a_value_that_is_not_a_literal() {
    let nested: Expression<Value> = Expression::new(
        "{} = {}",
        vec![
            ExpressiveEnum::Nested(Expression::new("parent", vec![])),
            ExpressiveEnum::Nested(Expression::new("other_column", vec![])),
        ],
    );
    assert_eq!(literal_equality(&nested), None);
}

// --- search -------------------------------------------------------------------

async fn people() -> MockTable {
    let src = MockTableSource::new()
        .with_data(
            "people",
            vec![
                json!({"id": "1", "name": "Alice", "note": "bob's friend"}),
                json!({"id": "2", "name": "Bob", "note": "quiet"}),
            ],
        )
        .await;
    MockTable::new("people", src)
}

async fn found(table: MockTable) -> Vec<String> {
    table.list_values().await.unwrap().into_keys().collect()
}

#[tokio::test]
async fn search_matches_a_case_insensitive_substring() {
    let table = people()
        .await
        .with_column_of::<String>("name")
        .with_search("LIC");
    assert_eq!(found(table).await, ["1"]);
}

#[tokio::test]
async fn search_reads_every_column_when_none_is_flagged_searchable() {
    let table = people()
        .await
        .with_column_of::<String>("note")
        .with_search("bob");
    assert_eq!(found(table).await, ["1"]);
}

#[tokio::test]
async fn search_reads_only_the_searchable_columns_once_any_is_flagged() {
    let table = people()
        .await
        .with_column_of::<String>("note")
        .with_column(MockColumn::<String>::new("name").with_flag(ColumnFlag::Searchable));
    assert_eq!(found(table.clone().with_search("bob")).await, ["2"]);
    assert!(found(table.with_search("quiet")).await.is_empty());
}

#[tokio::test]
async fn add_search_is_the_in_place_form_and_narrows_the_count() {
    let mut table = people().await.with_column_of::<String>("name");
    table.add_search("alice");
    assert_eq!(table.get_count().await.unwrap(), 1);
}

// --- ordering -----------------------------------------------------------------

#[test]
fn orders_are_listed_in_the_order_they_were_added_with_their_direction() {
    let mut table = table();
    table.add_order(expr!("name").ascending());
    table.add_order(expr!("age").descending());
    let orders: Vec<_> = table.orders().collect();
    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].0.preview(), "name");
    assert_eq!(orders[0].1, SortDirection::Ascending);
    assert_eq!(orders[1].0.preview(), "age");
    assert_eq!(orders[1].1, SortDirection::Descending);
}

#[test]
fn order_by_builds_ascending_and_descending_clauses() {
    assert_eq!(OrderBy::ascending(1).direction, SortDirection::Ascending);
    assert_eq!(OrderBy::descending(1).direction, SortDirection::Descending);
    assert_eq!(OrderBy::ascending("x").expression, "x");
}

#[test]
fn with_order_is_the_builder_form_of_add_order() {
    let table = table().with_order(OrderBy::descending(expr!("age")));
    assert_eq!(table.orders().count(), 1);
}

#[test]
fn a_temporary_order_can_be_removed_and_permanent_ones_stay() {
    let mut table = table();
    table.add_order(OrderBy::ascending(expr!("name")));
    let first = table.temp_add_order(OrderBy::descending(expr!("age")));
    let second = table.temp_add_order(OrderBy::ascending(expr!("created_at")));
    assert_eq!(table.orders().count(), 3);

    table.temp_remove_order(first).unwrap();
    assert_eq!(table.orders().count(), 2);
    table.add_order(OrderBy::ascending(expr!("id")));
    table.temp_remove_order(second).unwrap();
    let remaining: Vec<String> = table.orders().map(|(c, _)| c.preview()).collect();
    assert_eq!(remaining, ["name", "id"]);
}

#[test]
fn clear_orders_drops_permanent_and_temporary_orders() {
    let mut table = table();
    table.add_order(OrderBy::ascending(expr!("name")));
    table.temp_add_order(OrderBy::descending(expr!("age")));
    table.clear_orders();
    assert_eq!(table.orders().count(), 0);
}

// --- pagination ---------------------------------------------------------------

#[test]
fn pagination_new_counts_pages_from_one() {
    let p = Pagination::new(2, 25);
    assert_eq!(p.get_page(), 2);
    assert_eq!(p.get_ipp(), 25);
    assert_eq!(p.limit(), 25);
    assert_eq!(p.skip(), 25);
}

#[test]
fn pagination_never_goes_below_page_one_or_one_item_per_page() {
    let p = Pagination::new(0, 0);
    assert_eq!(p.get_page(), 1);
    assert_eq!(p.get_ipp(), 1);
    let mut q = Pagination::new(3, 10);
    q.set_page(0);
    assert_eq!(q.get_page(), 1);
    q.set_ipp(0);
    assert_eq!(q.get_ipp(), 1);
}

#[test]
fn set_page_moves_the_skip() {
    let mut p = Pagination::new(1, 10);
    p.set_page(3);
    assert_eq!(p.get_page(), 3);
    assert_eq!(p.skip(), 20);
}

#[test]
fn set_ipp_keeps_the_first_visible_item_visible() {
    let mut p = Pagination::new(3, 10);
    p.set_ipp(5);
    assert_eq!(p.get_page(), 5);
    assert_eq!(p.get_ipp(), 5);
    assert_eq!(p.skip(), 20);

    let mut q = Pagination::new(5, 5);
    q.set_ipp(25);
    assert_eq!(q.get_page(), 1);
    assert_eq!(q.skip(), 0);
}

#[test]
fn a_window_skips_its_offset_whatever_the_page_size() {
    let p = Pagination::window(7, 10);
    assert_eq!(p.skip(), 7);
    assert_eq!(p.limit(), 10);
}

#[test]
fn default_pagination_starts_at_the_first_page() {
    let p = Pagination::default();
    assert_eq!(p.get_page(), 1);
    assert_eq!(p.skip(), 0);
    assert_eq!(p.get_ipp(), 50);
}

#[test]
fn apply_on_select_sets_limit_and_skip() {
    let mut select = vantage_expressions::mocks::select::MockSelect::new();
    Pagination::new(3, 10).apply_on_select(&mut select);
    assert_eq!(select.get_limit(), Some(10));
    assert_eq!(select.get_skip(), Some(20));
}

#[test]
fn a_table_holds_its_pagination_until_it_is_cleared() {
    let mut table = table();
    assert!(table.pagination().is_none());
    table.set_pagination(Some(Pagination::new(2, 5)));
    assert_eq!(table.pagination(), Some(&Pagination::new(2, 5)));
    table.set_pagination(None);
    assert!(table.pagination().is_none());
}

// --- select -------------------------------------------------------------------

fn selectable(count: Value) -> MockTableSource {
    MockTableSource::new()
        .with_select_source(MockSelectableDataSource::new(json!([])))
        .with_query_source(
            mock_builder::new().on_exact_select("(SELECT COUNT(*) FROM \"users\")", count),
        )
}

fn users(count: Value) -> MockTable {
    MockTable::new("users", selectable(count))
}

#[test]
fn select_reads_the_table_and_projects_its_columns_in_declaration_order() {
    let table = users(json!(0))
        .with_column_of::<String>("name")
        .with_column_of::<i64>("age");
    let select = table.select();
    assert_eq!(select.source(), Some("users"));
    assert_eq!(select.fields(), ["name", "age"]);
}

#[test]
fn select_carries_the_tables_conditions() {
    let table = users(json!(0)).with_condition(expr_any!("age > 18"));
    let select = table.select();
    assert_eq!(select.where_conditions().len(), 1);
    assert_eq!(select.where_conditions()[0].preview(), "age > 18");
}

#[test]
fn select_carries_ordering_and_pagination() {
    let mut table = users(json!(0)).with_order(OrderBy::ascending(expr!("name")));
    let select = table.select();
    assert!(select.has_order_by());
    assert_eq!(select.get_limit(), None);

    table.set_pagination(Some(Pagination::new(3, 10)));
    let select = table.select();
    assert_eq!(select.get_limit(), Some(10));
    assert_eq!(select.get_skip(), Some(20));
}

#[test]
fn select_does_not_project_lazy_columns_because_the_source_has_no_such_field() {
    let table = users(json!(0))
        .with_column_of::<String>("name")
        .with_lazy_expression("greeting", |_| async { Ok(json!("hi")) });
    assert_eq!(table.select().fields(), ["name"]);
}

#[test]
fn a_lazy_column_stays_out_of_the_select_even_when_an_expression_shares_its_name() {
    let table = users(json!(0))
        .with_lazy_expression("tag", |_| async { Ok(json!("x")) })
        .with_expression("tag", |_| expr_any!("upper(name)"));
    assert!(table.select().fields().is_empty());
}

#[test]
fn select_empty_has_source_conditions_and_pagination_but_no_fields() {
    let mut table = users(json!(0))
        .with_column_of::<String>("name")
        .with_condition(expr_any!("age > 18"));
    table.set_pagination(Some(Pagination::new(2, 5)));
    let select = table.select_empty();
    assert_eq!(select.source(), Some("users"));
    assert!(select.fields().is_empty());
    assert_eq!(select.where_conditions().len(), 1);
    assert_eq!(select.get_limit(), Some(5));
    assert_eq!(select.get_skip(), Some(5));
}

#[test]
fn the_count_query_is_parenthesised_so_it_nests_as_a_subquery() {
    let preview = users(json!(0)).get_count_query().preview();
    assert!(
        preview.starts_with('(') && preview.ends_with(')'),
        "{preview}"
    );
    assert!(preview.contains("COUNT"), "{preview}");
    assert!(preview.contains("users"), "{preview}");
}

#[tokio::test]
async fn a_bare_count_result_is_the_row_count() {
    assert_eq!(users(json!(42)).get_count_via_query().await.unwrap(), 42);
}

#[tokio::test]
async fn a_count_object_result_is_the_row_count() {
    assert_eq!(
        users(json!({"count": 5}))
            .get_count_via_query()
            .await
            .unwrap(),
        5
    );
}

#[tokio::test]
async fn a_single_element_array_result_is_unwrapped() {
    assert_eq!(
        users(json!([{"count": 7}]))
            .get_count_via_query()
            .await
            .unwrap(),
        7
    );
    assert_eq!(users(json!([9])).get_count_via_query().await.unwrap(), 9);
}

#[tokio::test]
async fn an_unrecognised_count_result_is_an_error_not_zero() {
    assert!(
        users(json!({"total": 5}))
            .get_count_via_query()
            .await
            .is_err()
    );
}

// --- counts -------------------------------------------------------------------

#[tokio::test]
async fn get_count_counts_the_rows_and_follows_writes() {
    let table = people().await;
    assert_eq!(table.get_count().await.unwrap(), 2);
    table
        .insert_value("3", &Record::from(json!({"name": "Carol"})))
        .await
        .unwrap();
    assert_eq!(table.get_count().await.unwrap(), 3);
}

#[tokio::test]
async fn get_count_counts_only_the_rows_in_the_set() {
    let all = people().await;
    let condition = all
        .data_source()
        .eq_value_condition("name", json!("Alice"))
        .unwrap();
    let set = all.clone().with_condition(condition);
    assert_eq!(set.get_count().await.unwrap(), 1);
    assert_eq!(all.get_count().await.unwrap(), 2);
}

#[tokio::test]
async fn the_count_expression_executes_to_the_live_row_count() {
    let src = MockTableSource::new()
        .with_data("t", vec![json!({"id": "1"}), json!({"id": "2"})])
        .await
        .with_query_source(mock_builder::new());
    let table = MockTable::new("t", src);
    table
        .insert_value("3", &Record::from(json!({"name": "Carol"})))
        .await
        .unwrap();

    let count = table.get_expr_count();
    let raw = table
        .data_source()
        .execute(count.expression())
        .await
        .unwrap();
    assert_eq!(raw, json!(3));
}

// --- in_values_condition ------------------------------------------------------

#[tokio::test]
async fn in_values_condition_narrows_to_the_listed_values() {
    let src = MockTableSource::new()
        .with_data(
            "t",
            vec![
                json!({"id": "a", "code": "a"}),
                json!({"id": "b", "code": "b"}),
                json!({"id": "c", "code": "c"}),
            ],
        )
        .await;
    let condition = src
        .in_values_condition("code", vec![json!("a"), json!("c")])
        .unwrap();
    let table = MockTable::new("t", src).with_condition(condition);
    assert_eq!(found(table).await, ["a", "c"]);
}

#[test]
fn condition_column_names_an_in_values_condition() {
    let src = MockTableSource::new();
    let condition = src.in_values_condition("code", vec![json!("a")]).unwrap();
    assert_eq!(src.condition_column(&condition), Some("code".to_string()));
}

// --- read hook ----------------------------------------------------------------

fn refusing() -> Hook<MockTableSource> {
    Hook::BeforeSelect(select_hook(|_cx: SelectCx<'_, MockTableSource>| {
        Box::pin(async { Err(vantage_core::error!("read refused")) })
    }))
}

fn refused<X: std::fmt::Debug>(result: vantage_core::Result<X>) -> bool {
    matches!(result, Err(e) if e.to_string().contains("read refused"))
}

#[tokio::test]
async fn before_select_error_refuses_list_get_count_and_aggregates() {
    let table = people()
        .await
        .with_column_of::<i64>("age")
        .with_hook(refusing());
    let age = MockColumn::<vantage_table::mocks::mock_type_system::AnyMockType>::new("age");

    assert!(refused(table.list_values().await));
    assert!(refused(table.get_value("1").await));
    assert!(refused(table.get_some_value().await));
    assert!(refused(table.list().await));
    assert!(refused(table.get("1").await));
    assert!(refused(table.get_count().await));
    assert!(refused(table.get_sum(&age).await));
    assert!(refused(table.get_max(&age).await));
    assert!(refused(table.get_min(&age).await));
    assert!(refused(table.check_read().await));
}

#[tokio::test]
async fn before_select_error_is_the_first_item_of_a_stream() {
    let table = people().await.with_hook(refusing());
    let mut values = table.stream_values();
    let first = std::future::poll_fn(|cx| values.as_mut().poll_next(cx)).await;
    assert!(refused(first.unwrap()));
    assert!(
        std::future::poll_fn(|cx| values.as_mut().poll_next(cx))
            .await
            .is_none()
    );

    let mut entities = table.stream();
    let first = std::future::poll_fn(|cx| entities.as_mut().poll_next(cx)).await;
    assert!(refused(first.unwrap()));
}

#[tokio::test]
async fn before_select_sees_the_set_and_the_context_and_ok_lets_the_read_through() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    let hook = select_hook(move |cx: SelectCx<'_, MockTableSource>| {
        let log = log.clone();
        let group = cx.context.and_then(|c| c.group.clone());
        let conditions = cx.table.conditions().count();
        Box::pin(async move {
            log.lock().unwrap().push((group, conditions));
            Ok(())
        })
    });
    let all = people().await;
    let alice = all
        .data_source()
        .eq_value_condition("name", json!("Alice"))
        .unwrap();
    let table = all
        .with_condition(alice)
        .with_hook(Hook::BeforeSelect(hook));

    let context = vantage_table::write_context::WriteContext::new().with_group("g1");
    let rows = context.scope(table.list_values()).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(table.get_count().await.unwrap(), 1);
    assert_eq!(
        *seen.lock().unwrap(),
        [(Some("g1".to_string()), 1), (None, 1)]
    );
}

#[test]
fn before_select_does_not_run_when_the_table_is_only_built_into_an_expression() {
    let table = users(json!(0)).with_hook(refusing());
    assert!(table.get_count_query().preview().contains("COUNT"));
}
