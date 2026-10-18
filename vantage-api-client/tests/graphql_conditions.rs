//! `GraphqlCondition` rendered into a filter argument object, per dialect:
//! Hasura spells operators (`_eq`, `_gt`, ...) and groups (`_and`, `_or`,
//! `_not`); Generic is one flat object and equality only.

use serde_json::{Value, json};
use vantage_api_client::{
    AnyGraphqlType, FieldCondition, FilterDialect, GraphqlApi, GraphqlCondition, GraphqlOp,
    GraphqlOperation,
};
use vantage_expressions::{DeferredFn, ExpressiveEnum};
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::EmptyEntity;

fn field(name: &str, op: GraphqlOp, value: Value) -> GraphqlCondition {
    GraphqlCondition::Field(FieldCondition::new(name, op, value))
}

async fn hasura(condition: &GraphqlCondition) -> Value {
    condition.render(FilterDialect::Hasura).await.unwrap()
}

async fn generic_error(condition: &GraphqlCondition) -> String {
    condition
        .render(FilterDialect::Generic)
        .await
        .unwrap_err()
        .to_string()
}

fn deferred_scalar(value: AnyGraphqlType) -> DeferredFn<AnyGraphqlType> {
    DeferredFn::new(move || {
        let value = value.clone();
        Box::pin(async move { Ok(ExpressiveEnum::Scalar(value)) })
    })
}

#[tokio::test]
async fn hasura_wraps_each_operator_in_its_underscore_key() {
    let cases = [
        (GraphqlOp::Eq, json!("x"), "_eq"),
        (GraphqlOp::Ne, json!("x"), "_neq"),
        (GraphqlOp::Gt, json!(1), "_gt"),
        (GraphqlOp::Gte, json!(1), "_gte"),
        (GraphqlOp::Lt, json!(1), "_lt"),
        (GraphqlOp::Lte, json!(1), "_lte"),
        (GraphqlOp::In, json!([1, 2]), "_in"),
        (GraphqlOp::NotIn, json!([1, 2]), "_nin"),
        (GraphqlOp::Like, json!("a%"), "_like"),
        (GraphqlOp::ILike, json!("a%"), "_ilike"),
    ];
    for (op, value, key) in cases {
        let rendered = hasura(&field("f", op.clone(), value.clone())).await;
        assert_eq!(rendered, json!({ "f": { (key): value } }), "{op:?}");
        assert_eq!(op.hasura_key(), Some(key));
    }
}

#[tokio::test]
async fn hasura_null_checks_take_a_bool() {
    let null = hasura(&field("deleted_at", GraphqlOp::IsNull, Value::Null)).await;
    assert_eq!(null, json!({ "deleted_at": { "_is_null": true } }));

    let not_null = hasura(&field("deleted_at", GraphqlOp::IsNotNull, Value::Null)).await;
    assert_eq!(not_null, json!({ "deleted_at": { "_is_null": false } }));
}

#[tokio::test]
async fn generic_renders_equality_as_a_flat_field() {
    let rendered = field("mission_name", GraphqlOp::Eq, json!("FalconSat"))
        .render(FilterDialect::Generic)
        .await
        .unwrap();
    assert_eq!(rendered, json!({ "mission_name": "FalconSat" }));
}

#[tokio::test]
async fn generic_refuses_every_operator_but_equality() {
    for op in [
        GraphqlOp::Ne,
        GraphqlOp::Gt,
        GraphqlOp::Gte,
        GraphqlOp::Lt,
        GraphqlOp::Lte,
        GraphqlOp::In,
        GraphqlOp::NotIn,
        GraphqlOp::Like,
        GraphqlOp::ILike,
        GraphqlOp::IsNull,
        GraphqlOp::IsNotNull,
    ] {
        let err = generic_error(&field("price", op.clone(), json!(100))).await;
        assert!(err.contains("Generic"), "{op:?}: {err}");
    }
}

#[test]
fn describe_condition_reads_as_the_expression() {
    let api = GraphqlApi::new("https://example.test/graphql");
    let cond = api
        .eq_value_condition("parent", AnyGraphqlType::from("p1"))
        .unwrap();
    assert_eq!(api.describe_condition(&cond), "parent = \"p1\"");
}

#[test]
fn condition_column_names_eq_and_in() {
    let api = GraphqlApi::new("https://example.test/graphql");
    let eq = api
        .eq_value_condition("parent", AnyGraphqlType::from("p1"))
        .unwrap();
    assert_eq!(api.condition_column(&eq), Some("parent".to_string()));

    let invoices =
        Table::<GraphqlApi, EmptyEntity>::new("invoice", api.clone()).with_id_column("id");
    let in_cond = api.related_in_condition("invoice_id", &invoices, "id");
    assert_eq!(
        api.condition_column(&in_cond),
        Some("invoice_id".to_string())
    );
}

#[test]
fn condition_column_names_an_in_values_condition() {
    let api = GraphqlApi::new("https://example.test/graphql");
    let cond = api
        .in_values_condition("invoice_id", vec![AnyGraphqlType::from("i1")])
        .unwrap();
    assert_eq!(api.condition_column(&cond), Some("invoice_id".to_string()));
}

#[test]
fn condition_column_is_none_for_raw_expressions() {
    let api = GraphqlApi::new("https://example.test/graphql");
    let both = GraphqlCondition::And(vec![
        field("name", GraphqlOp::Eq, json!("Alice")),
        field("active", GraphqlOp::Eq, json!(true)),
    ]);
    assert_eq!(api.condition_column(&both), None);
    let not = GraphqlCondition::Not(Box::new(field("active", GraphqlOp::Eq, json!(true))));
    assert_eq!(api.condition_column(&not), None);
    let or = GraphqlCondition::Or(vec![
        field("tenant_id", GraphqlOp::Eq, json!("acme")),
        field("id", GraphqlOp::Gte, json!(0)),
    ]);
    assert_eq!(api.condition_column(&or), None);
}

#[test]
fn condition_column_names_a_comparison() {
    let api = GraphqlApi::new("https://example.test/graphql");
    let gt = field("invoice_id", GraphqlOp::Gt, json!(1));
    assert_eq!(api.condition_column(&gt), Some("invoice_id".to_string()));
}

#[test]
fn condition_column_is_none_for_negations() {
    let api = GraphqlApi::new("https://example.test/graphql");
    for op in [
        GraphqlOp::Ne,
        GraphqlOp::NotIn,
        GraphqlOp::IsNull,
        GraphqlOp::IsNotNull,
    ] {
        let cond = field("tenant_id", op.clone(), json!("acme"));
        assert_eq!(api.condition_column(&cond), None, "{op:?}");
    }
    let deferred = GraphqlCondition::deferred_field(
        "tenant_id",
        GraphqlOp::Ne,
        deferred_scalar(AnyGraphqlType::new("acme".to_string())),
    );
    assert_eq!(api.condition_column(&deferred), None);
}

#[test]
fn the_dialect_defaults_to_generic() {
    assert_eq!(FilterDialect::default(), FilterDialect::Generic);
}

#[tokio::test]
async fn hasura_and_of_distinct_fields_is_one_object() {
    let both = GraphqlCondition::And(vec![
        field("name", GraphqlOp::Eq, json!("Alice")),
        field("active", GraphqlOp::Eq, json!(true)),
    ]);
    assert_eq!(
        hasura(&both).await,
        json!({ "name": { "_eq": "Alice" }, "active": { "_eq": true } })
    );
}

#[tokio::test]
async fn hasura_and_of_one_field_twice_uses_an_explicit_and() {
    let range = GraphqlCondition::And(vec![
        field("price", GraphqlOp::Gt, json!(10)),
        field("price", GraphqlOp::Lt, json!(100)),
    ]);
    assert_eq!(
        hasura(&range).await,
        json!({ "_and": [
            { "price": { "_gt": 10 } },
            { "price": { "_lt": 100 } }
        ] })
    );
}

#[tokio::test]
async fn generic_and_of_distinct_fields_is_one_flat_object() {
    let both = GraphqlCondition::And(vec![
        field("name", GraphqlOp::Eq, json!("Alice")),
        field("active", GraphqlOp::Eq, json!(true)),
    ]);
    let rendered = both.render(FilterDialect::Generic).await.unwrap();
    assert_eq!(rendered, json!({ "name": "Alice", "active": true }));
}

#[tokio::test]
async fn generic_cannot_express_two_conditions_on_one_field() {
    let clash = GraphqlCondition::And(vec![
        field("price", GraphqlOp::Eq, json!(10)),
        field("price", GraphqlOp::Eq, json!(20)),
    ]);
    let err = generic_error(&clash).await;
    assert!(err.contains("price"), "{err}");
}

#[tokio::test]
async fn hasura_groups_with_or_and_not() {
    let condition = GraphqlCondition::Not(Box::new(GraphqlCondition::Or(vec![
        field("active", GraphqlOp::Eq, json!(true)),
        field("count", GraphqlOp::Gt, json!(0)),
    ])));
    assert_eq!(
        hasura(&condition).await,
        json!({ "_not": { "_or": [
            { "active": { "_eq": true } },
            { "count": { "_gt": 0 } }
        ] } })
    );
}

#[tokio::test]
async fn generic_refuses_or_and_not() {
    let or = GraphqlCondition::Or(vec![
        field("a", GraphqlOp::Eq, json!(1)),
        field("b", GraphqlOp::Eq, json!(2)),
    ]);
    let err = generic_error(&or).await;
    assert!(err.contains("OR"), "{err}");

    let not = GraphqlCondition::Not(Box::new(field("a", GraphqlOp::Eq, json!(1))));
    let err = generic_error(&not).await;
    assert!(err.contains("NOT"), "{err}");
}

#[tokio::test]
async fn a_deferred_field_is_resolved_when_rendered() {
    let condition = GraphqlCondition::deferred_field(
        "launch_id",
        GraphqlOp::Eq,
        deferred_scalar(AnyGraphqlType::new("abc-123".to_string())),
    );
    assert_eq!(
        hasura(&condition).await,
        json!({ "launch_id": { "_eq": "abc-123" } })
    );
    let flat = condition.render(FilterDialect::Generic).await.unwrap();
    assert_eq!(flat, json!({ "launch_id": "abc-123" }));

    let numeric = GraphqlCondition::deferred_field(
        "launch_id",
        GraphqlOp::Eq,
        deferred_scalar(AnyGraphqlType::new(7i64)),
    );
    let flat = numeric.render(FilterDialect::Generic).await.unwrap();
    assert_eq!(flat, json!({ "launch_id": 7 }));
}

#[tokio::test]
async fn a_deferred_field_resolving_to_a_non_scalar_is_an_error() {
    let nested = DeferredFn::new(|| {
        Box::pin(async {
            Ok(ExpressiveEnum::Nested(
                vantage_expressions::Expression::new("x", vec![]),
            ))
        })
    });
    let condition = GraphqlCondition::deferred_field("launch_id", GraphqlOp::Eq, nested);
    let err = condition
        .render(FilterDialect::Hasura)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("non-scalar"), "{err}");
}

#[tokio::test]
async fn a_deferred_condition_must_resolve_to_a_filter_object() {
    let object = GraphqlCondition::Deferred(deferred_scalar(AnyGraphqlType::untyped(
        json!({ "id": { "_eq": 1 } }),
    )));
    assert_eq!(hasura(&object).await, json!({ "id": { "_eq": 1 } }));

    let scalar = GraphqlCondition::Deferred(deferred_scalar(AnyGraphqlType::new(1i64)));
    let err = scalar
        .render(FilterDialect::Hasura)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("JSON object"), "{err}");
}

#[test]
fn the_preview_renders_literal_conditions_like_the_real_thing() {
    let condition = GraphqlCondition::And(vec![
        field("name", GraphqlOp::Eq, json!("Alice")),
        field("age", GraphqlOp::Gt, json!(30)),
    ]);
    assert_eq!(
        condition.render_preview(FilterDialect::Hasura).unwrap(),
        json!({ "name": { "_eq": "Alice" }, "age": { "_gt": 30 } })
    );
    assert_eq!(
        field("name", GraphqlOp::Eq, json!("Alice"))
            .render_preview(FilterDialect::Generic)
            .unwrap(),
        json!({ "name": "Alice" })
    );

    let group = GraphqlCondition::Not(Box::new(GraphqlCondition::Or(vec![field(
        "a",
        GraphqlOp::Eq,
        json!(1),
    )])));
    assert_eq!(
        group.render_preview(FilterDialect::Hasura).unwrap(),
        json!({ "_not": { "_or": [{ "a": { "_eq": 1 } }] } })
    );
}

#[test]
fn the_preview_refuses_what_generic_cannot_spell() {
    let or = GraphqlCondition::Or(vec![field("a", GraphqlOp::Eq, json!(1))]);
    let err = or.render_preview(FilterDialect::Generic).unwrap_err();
    assert!(err.to_string().contains("OR"), "{err}");

    let not = GraphqlCondition::Not(Box::new(field("a", GraphqlOp::Eq, json!(1))));
    let err = not.render_preview(FilterDialect::Generic).unwrap_err();
    assert!(err.to_string().contains("NOT"), "{err}");
}

#[test]
fn the_preview_marks_deferred_values_instead_of_resolving_them() {
    let deferred_field = GraphqlCondition::deferred_field(
        "stack_id",
        GraphqlOp::Eq,
        deferred_scalar(AnyGraphqlType::new("x".to_string())),
    );
    let marker = deferred_field
        .render_preview(FilterDialect::Hasura)
        .unwrap();
    assert!(
        marker.as_str().unwrap().starts_with("**deferred("),
        "{marker}"
    );
    assert!(marker.as_str().unwrap().contains("stack_id"), "{marker}");

    let deferred = GraphqlCondition::Deferred(deferred_scalar(AnyGraphqlType::new(1i64)));
    let marker = deferred.render_preview(FilterDialect::Hasura).unwrap();
    assert!(
        marker.as_str().unwrap().starts_with("**deferred("),
        "{marker}"
    );
}

#[tokio::test]
async fn column_operators_build_conditions_that_render_per_dialect() {
    let mission = Column::<String>::new("mission_name");
    let eq = mission.eq("FalconSat");
    assert_eq!(
        hasura(&eq).await,
        json!({ "mission_name": { "_eq": "FalconSat" } })
    );
    assert_eq!(
        eq.render(FilterDialect::Generic).await.unwrap(),
        json!({ "mission_name": "FalconSat" })
    );

    let price = Column::<i64>::new("price");
    assert_eq!(
        hasura(&price.ne(5i64)).await,
        json!({ "price": { "_neq": 5 } })
    );
    assert_eq!(
        hasura(&price.gt(100i64)).await,
        json!({ "price": { "_gt": 100 } })
    );
    assert_eq!(
        hasura(&price.gte(100i64)).await,
        json!({ "price": { "_gte": 100 } })
    );
    assert_eq!(
        hasura(&price.lt(100i64)).await,
        json!({ "price": { "_lt": 100 } })
    );
    assert_eq!(
        hasura(&price.lte(100i64)).await,
        json!({ "price": { "_lte": 100 } })
    );
}

#[tokio::test]
async fn column_set_and_pattern_operators_render_their_arguments() {
    let status = Column::<String>::new("status");
    assert_eq!(
        hasura(&status.in_(vec!["active", "pending"])).await,
        json!({ "status": { "_in": ["active", "pending"] } })
    );
    assert_eq!(
        hasura(&status.not_in(vec!["gone"])).await,
        json!({ "status": { "_nin": ["gone"] } })
    );
    assert_eq!(
        hasura(&status.like("act%")).await,
        json!({ "status": { "_like": "act%" } })
    );
    assert_eq!(
        hasura(&status.ilike("%ACT%")).await,
        json!({ "status": { "_ilike": "%ACT%" } })
    );
    assert_eq!(
        hasura(&status.is_null()).await,
        json!({ "status": { "_is_null": true } })
    );
    assert_eq!(
        hasura(&status.is_not_null()).await,
        json!({ "status": { "_is_null": false } })
    );
}

#[tokio::test]
async fn a_field_condition_converts_into_a_condition() {
    let condition: GraphqlCondition = FieldCondition::new("a", GraphqlOp::Eq, json!(1)).into();
    assert_eq!(hasura(&condition).await, json!({ "a": { "_eq": 1 } }));
    assert_eq!(
        hasura(&GraphqlCondition::eq("a", 1)).await,
        json!({ "a": { "_eq": 1 } })
    );
}
