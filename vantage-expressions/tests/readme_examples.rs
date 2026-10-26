//! Expression composition, flattening and type mapping.

use serde_json::{Value, json};
use vantage_expressions::{
    DeferredFn, Expression, ExpressionFlattener, ExpressionMap, Expressive, ExpressiveEnum,
    Flatten, expr, expr_any, expr_as,
};

#[test]
fn macros_build_template_and_parameters_and_preview_fills_each_once() {
    let json_expr = expr!("age > {}", 18);
    assert_eq!(json_expr.template, "age > {}");
    assert_eq!(json_expr.parameters.len(), 1);
    assert_eq!(json_expr.preview(), "age > 18");

    let typed = expr_as!(String, "{} = {}", "weird{}", "x");
    assert_eq!(typed.preview(), "weird{} = x");

    let inferred: Expression<i16> = expr_any!("age > {}", 18i16);
    assert_eq!(inferred.template, "age > {}");
    assert_eq!(inferred.parameters.len(), 1);

    let plain = expr!("SELECT 1");
    assert!(plain.parameters.is_empty());
}

#[test]
fn parenthesised_macro_argument_is_nested() {
    let inner = expr!("status = {}", "active");
    let outer = expr!("WHERE {} AND age > {}", (inner), 21);

    assert!(matches!(outer.parameters[0], ExpressiveEnum::Nested(_)));
    assert!(matches!(outer.parameters[1], ExpressiveEnum::Scalar(_)));
    assert_eq!(outer.preview(), "WHERE status = \"active\" AND age > 21");
}

#[test]
fn debug_output_is_the_preview() {
    let expr = expr!("SELECT * FROM users WHERE id = {}", 7);
    assert_eq!(format!("{:?}", expr), expr.preview());
}

#[test]
fn expression_is_expressive_and_expr_returns_an_equal_copy() {
    let expr = expr!("age > {}", 18);
    let again = expr.expr();
    assert_eq!(again.template, expr.template);
    assert_eq!(again.preview(), expr.preview());
    assert_eq!(Expressive::preview(&expr), "age > 18");
}

#[test]
fn from_vec_joins_expressions_with_the_delimiter() {
    let conditions = vec![
        expr!("age >= {}", 25),
        expr!("status = {}", "premium"),
        expr!("last_login > NOW() - INTERVAL 30 DAY"),
    ];
    let where_clause = Expression::from_vec(conditions, " AND ");

    assert_eq!(where_clause.template, "{} AND {} AND {}");
    assert_eq!(where_clause.parameters.len(), 3);

    let query = expr!("SELECT * FROM users WHERE {}", (where_clause));
    assert_eq!(
        query.preview(),
        "SELECT * FROM users WHERE age >= 25 AND status = \"premium\" AND last_login > NOW() - INTERVAL 30 DAY"
    );

    let flattened = ExpressionFlattener::new().flatten(&query);
    assert_eq!(
        flattened.template,
        "SELECT * FROM users WHERE age >= {} AND status = {} AND last_login > NOW() - INTERVAL 30 DAY"
    );
    assert_eq!(flattened.parameters.len(), 2);
    assert_eq!(flattened.preview(), query.preview());
}

#[test]
fn from_vec_of_nothing_and_empty_render_as_nothing() {
    let none = Expression::<Value>::from_vec(vec![], ", ");
    assert_eq!(none.template, "");
    assert!(none.parameters.is_empty());
    assert_eq!(none.preview(), "");

    assert_eq!(Expression::<Value>::empty().preview(), "");
}

#[test]
fn flatten_inlines_nested_expressions_in_order() {
    let flattener = ExpressionFlattener::new();

    let greeting = expr_as!(String, "Hello {}", "John");
    let farewell = expr_as!(String, "Goodbye {}", "Jane");
    let main_expr = expr_as!(String, "{} and {}", (greeting), (farewell));
    let flattened = flattener.flatten(&main_expr);
    assert_eq!(flattened.template, "Hello {} and Goodbye {}");
    assert_eq!(flattened.preview(), "Hello John and Goodbye Jane");
    assert_eq!(main_expr.template, "{} and {}");

    let count = expr_as!(String, "count({})", "*");
    let mixed = expr_as!(String, "SELECT {} FROM users WHERE age > {}", (count), "25");
    let flattened = flattener.flatten(&mixed);
    assert_eq!(
        flattened.template,
        "SELECT count({}) FROM users WHERE age > {}"
    );
    assert_eq!(flattened.parameters.len(), 2);
}

#[test]
fn flatten_resolves_every_level_of_nesting() {
    let level3 = expr!("c = {}", 3);
    let level2 = expr!("b = {} AND {}", 2, (level3));
    let level1 = expr!("a = {} AND {}", 1, (level2));
    let query = expr!("SELECT * FROM t WHERE {}", (level1));

    let flattened = ExpressionFlattener.flatten(&query);
    assert_eq!(
        flattened.template,
        "SELECT * FROM t WHERE a = {} AND b = {} AND c = {}"
    );
    assert_eq!(flattened.parameters.len(), 3);
    assert!(
        flattened
            .parameters
            .iter()
            .all(|p| matches!(p, ExpressiveEnum::Scalar(_)))
    );
    assert_eq!(
        flattened.preview(),
        "SELECT * FROM t WHERE a = 1 AND b = 2 AND c = 3"
    );
}

#[test]
fn flatten_nested_matches_flatten_and_keeps_deferred_parameters() {
    let flattener = ExpressionFlattener::new();
    let deferred =
        DeferredFn::<Value>::new(|| Box::pin(async { Ok(ExpressiveEnum::Scalar(json!(1))) }));
    let inner = expr!("x = {}", { deferred });
    let outer = expr!("WHERE {}", (inner));

    let flattened = flattener.flatten_nested(&outer);
    assert_eq!(flattened.template, "WHERE x = {}");
    assert!(matches!(
        flattened.parameters[0],
        ExpressiveEnum::Deferred(_)
    ));
    assert_eq!(flattened.preview(), "WHERE x = **deferred()");
}

#[test]
fn map_converts_scalar_parameters() {
    let string_expr: Expression<String> = Expression::new(
        "SELECT * FROM users WHERE name = {}",
        vec![ExpressiveEnum::Scalar("John".to_string())],
    );
    let value_expr: Expression<Value> = string_expr.map();

    assert_eq!(value_expr.template, "SELECT * FROM users WHERE name = {}");
    match &value_expr.parameters[0] {
        ExpressiveEnum::Scalar(v) => assert_eq!(v, &json!("John")),
        _ => panic!("expected a scalar parameter"),
    }
}

#[test]
fn map_converts_nested_expressions_recursively() {
    let inner: Expression<String> = Expression::new(
        "status = {}",
        vec![ExpressiveEnum::Scalar("active".to_string())],
    );
    let outer: Expression<String> = Expression::new(
        "SELECT * FROM users WHERE {}",
        vec![ExpressiveEnum::Nested(inner)],
    );

    let mapped: Expression<Value> = outer.map();
    assert_eq!(mapped.template, "SELECT * FROM users WHERE {}");
    match &mapped.parameters[0] {
        ExpressiveEnum::Nested(n) => {
            assert_eq!(n.template, "status = {}");
            assert_eq!(n.preview(), "status = \"active\"");
        }
        _ => panic!("expected a nested parameter"),
    }
}

#[tokio::test]
async fn map_converts_what_a_deferred_parameter_returns() {
    let scalar = DeferredFn::<String>::new(|| {
        Box::pin(async { Ok(ExpressiveEnum::Scalar("test".to_string())) })
    });
    let nested = DeferredFn::<String>::new(|| {
        Box::pin(async {
            Ok(ExpressiveEnum::Nested(Expression::new(
                "id = {}",
                vec![ExpressiveEnum::Scalar("7".to_string())],
            )))
        })
    });
    let failing = DeferredFn::<String>::new(|| {
        Box::pin(async { Err(vantage_core::error!("lookup failed")) })
    });
    let expr: Expression<String> = Expression::new(
        "{} {} {}",
        vec![
            ExpressiveEnum::Deferred(scalar),
            ExpressiveEnum::Deferred(nested),
            ExpressiveEnum::Deferred(failing),
        ],
    );

    let mapped: Expression<Value> = expr.map();

    let ExpressiveEnum::Deferred(first) = &mapped.parameters[0] else {
        panic!("expected a deferred parameter");
    };
    match first.call().await.unwrap() {
        ExpressiveEnum::Scalar(v) => assert_eq!(v, json!("test")),
        _ => panic!("expected a scalar result"),
    }

    let ExpressiveEnum::Deferred(second) = &mapped.parameters[1] else {
        panic!("expected a deferred parameter");
    };
    match second.call().await.unwrap() {
        ExpressiveEnum::Nested(n) => assert_eq!(n.preview(), "id = \"7\""),
        _ => panic!("expected a nested result"),
    }

    let ExpressiveEnum::Deferred(third) = &mapped.parameters[2] else {
        panic!("expected a deferred parameter");
    };
    let err = third.call().await.unwrap_err();
    assert!(err.to_string().contains("lookup failed"));
}

#[test]
fn as_type_converts_scalars_like_map() {
    let ints: Expression<i64> = Expression::new("price > {}", vec![ExpressiveEnum::Scalar(10i64)]);
    let values = ints.as_type::<Value>();

    assert_eq!(values.template, "price > {}");
    assert_eq!(values.preview(), "price > 10");
}
