//! Resolving deferred parameters: `resolve_deferred`, `resolve_param`,
//! `execute_by_resolving` and `execute_strict`.

use vantage_expressions::{
    DeferredFn, Expression, ExpressiveEnum, execute_by_resolving, execute_strict, resolve_param,
};

fn from_template(template: &str) -> String {
    format!("tpl:{template}")
}

fn scalar(v: &str) -> ExpressiveEnum<String> {
    ExpressiveEnum::Scalar(v.to_string())
}

fn nested(template: &str, params: Vec<ExpressiveEnum<String>>) -> ExpressiveEnum<String> {
    ExpressiveEnum::Nested(Expression::new(template, params))
}

fn deferred_to(param: ExpressiveEnum<String>) -> ExpressiveEnum<String> {
    ExpressiveEnum::Deferred(DeferredFn::new(move || {
        let param = param.clone();
        Box::pin(async move { Ok(param) })
    }))
}

fn failing() -> ExpressiveEnum<String> {
    ExpressiveEnum::Deferred(DeferredFn::new(|| {
        Box::pin(async { Err(vantage_core::error!("lookup failed")) })
    }))
}

#[tokio::test]
async fn resolve_deferred_replaces_deferred_parameters_at_every_depth() {
    let seven = || deferred_to(scalar("7"));
    let inner = Expression::new("b = {}", vec![seven()]);
    let expr = Expression::new(
        "a = {} AND {} AND c = {}",
        vec![seven(), ExpressiveEnum::Nested(inner), scalar("keep")],
    );

    let resolved = expr.resolve_deferred().await.unwrap();

    assert_eq!(resolved.template, "a = {} AND {} AND c = {}");
    assert_eq!(resolved.preview(), "a = 7 AND b = 7 AND c = keep");
    assert_eq!(
        expr.preview(),
        "a = **deferred() AND b = **deferred() AND c = keep"
    );
}

#[tokio::test]
async fn resolve_deferred_fails_with_the_failing_call() {
    let inner = Expression::new("{}", vec![failing()]);
    let expr = Expression::new(
        "a = {} AND {}",
        vec![scalar("1"), ExpressiveEnum::Nested(inner)],
    );

    let err = expr.resolve_deferred().await.unwrap_err();

    assert!(err.to_string().contains("lookup failed"));
}

#[tokio::test]
async fn resolve_param_collapses_to_one_value() {
    assert_eq!(
        resolve_param(&scalar("v"), from_template).await.unwrap(),
        "v"
    );

    let chain = deferred_to(deferred_to(scalar("deep")));
    assert_eq!(resolve_param(&chain, from_template).await.unwrap(), "deep");

    let through_nested = deferred_to(nested("{}", vec![deferred_to(scalar("inner"))]));
    assert_eq!(
        resolve_param(&through_nested, from_template).await.unwrap(),
        "inner"
    );

    let single = nested("{}", vec![scalar("only")]);
    assert_eq!(resolve_param(&single, from_template).await.unwrap(), "only");
}

#[tokio::test]
async fn resolve_param_builds_from_the_template_without_a_single_parameter() {
    let none = nested("raw", vec![]);
    assert_eq!(
        resolve_param(&none, from_template).await.unwrap(),
        "tpl:raw"
    );

    let two = nested("{} {}", vec![scalar("a"), scalar("b")]);
    assert_eq!(
        resolve_param(&two, from_template).await.unwrap(),
        "tpl:{} {}"
    );
}

#[tokio::test]
async fn resolve_param_propagates_a_failing_deferred() {
    let err = resolve_param(&failing(), from_template).await.unwrap_err();
    assert!(err.to_string().contains("lookup failed"));
}

#[tokio::test]
async fn execute_by_resolving_hands_many_parameters_to_the_caller() {
    let none = Expression::<String>::new("raw", vec![]);
    let one = Expression::new("{}", vec![deferred_to(scalar("x"))]);
    let two = Expression::new("{} {}", vec![scalar("a"), scalar("b")]);
    let many = |params: &[ExpressiveEnum<String>]| {
        let n = params.len();
        async move { Ok(format!("many:{n}")) }
    };

    let result = execute_by_resolving(&none, from_template, many).await;
    assert_eq!(result.unwrap(), "tpl:raw");

    let result = execute_by_resolving(&one, from_template, many).await;
    assert_eq!(result.unwrap(), "x");

    let result = execute_by_resolving(&two, from_template, many).await;
    assert_eq!(result.unwrap(), "many:2");
}

#[tokio::test]
async fn execute_strict_unwraps_one_level() {
    let none = Expression::<String>::new("raw", vec![]);
    let result = execute_strict(&none, from_template, "test").await;
    assert_eq!(result.unwrap(), "tpl:raw");

    let plain = Expression::new("{}", vec![scalar("v")]);
    let result = execute_strict(&plain, from_template, "test").await;
    assert_eq!(result.unwrap(), "v");

    let deferred = Expression::new("{}", vec![deferred_to(scalar("d"))]);
    let result = execute_strict(&deferred, from_template, "test").await;
    assert_eq!(result.unwrap(), "d");

    let wrapped = Expression::new("{}", vec![nested("{}", vec![scalar("n")])]);
    let result = execute_strict(&wrapped, from_template, "test").await;
    assert_eq!(result.unwrap(), "n");

    let wrapped_deferred =
        Expression::new("{}", vec![nested("{}", vec![deferred_to(scalar("nd"))])]);
    let result = execute_strict(&wrapped_deferred, from_template, "test").await;
    assert_eq!(result.unwrap(), "nd");
}

#[tokio::test]
async fn execute_strict_rejects_every_other_shape() {
    let many = Expression::new("{} {}", vec![scalar("a"), scalar("b")]);
    let two_level = Expression::new(
        "{}",
        vec![nested("{}", vec![nested("{}", vec![scalar("v")])])],
    );
    let empty_nested = Expression::new("{}", vec![nested("raw", vec![])]);
    let wide_nested = Expression::new("{}", vec![nested("{} {}", vec![scalar("a"), scalar("b")])]);
    let deferred_to_nested =
        Expression::new("{}", vec![deferred_to(nested("{}", vec![scalar("v")]))]);
    let failing_deferred = Expression::new("{}", vec![failing()]);

    for expr in [
        many,
        two_level,
        empty_nested,
        wide_nested,
        deferred_to_nested,
        failing_deferred,
    ] {
        assert!(execute_strict(&expr, from_template, "test").await.is_err());
    }
}
