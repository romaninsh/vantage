//! Deferred and shared values in expressions.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use vantage_expressions::{
    DeferredFn, ExprDataSource, Expression, ExpressiveEnum, expr, mocks::mock_builder,
};

#[tokio::test]
async fn from_mutex_reads_the_value_at_call_time() {
    let counter = Arc::new(Mutex::new(10i32));
    let deferred = DeferredFn::<Value>::from_mutex(counter.clone());
    let query = expr!("SELECT * FROM items LIMIT {}", { deferred });

    *counter.lock().unwrap() = 25;

    assert_eq!(query.preview(), "SELECT * FROM items LIMIT **deferred()");
    let ExpressiveEnum::Deferred(deferred) = &query.parameters[0] else {
        panic!("expected a deferred parameter");
    };
    match deferred.call().await.unwrap() {
        ExpressiveEnum::Scalar(v) => assert_eq!(v, json!(25)),
        _ => panic!("expected a scalar result"),
    }

    *counter.lock().unwrap() = 3;
    match deferred.call().await.unwrap() {
        ExpressiveEnum::Scalar(v) => assert_eq!(v, json!(3)),
        _ => panic!("expected a scalar result"),
    }
}

#[tokio::test]
async fn executing_one_query_twice_sees_the_changed_shared_value() {
    let mock = mock_builder::new()
        .with_flattening()
        .on_exact_select(
            "SELECT event FROM almanac LIMIT 1",
            json!([{"event": "Cubs win World Series"}]),
        )
        .on_exact_select(
            "SELECT event FROM almanac LIMIT 2",
            json!([
                {"event": "Cubs win World Series"},
                {"event": "Florida Gators beat Miami"}
            ]),
        );
    let limit = Arc::new(Mutex::new(1i32));
    let query = expr!("SELECT event FROM almanac LIMIT {}", {
        DeferredFn::from_mutex(limit.clone())
    });

    let first = mock.execute(&query).await.unwrap();
    assert_eq!(first.as_array().unwrap().len(), 1);

    *limit.lock().unwrap() = 2;
    let second = mock.execute(&query).await.unwrap();
    assert_eq!(second.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn from_fn_wraps_an_async_function_result_and_error() {
    async fn forty_two() -> vantage_core::Result<i32> {
        Ok(42)
    }
    async fn broken() -> vantage_core::Result<i32> {
        Err(vantage_core::error!("API connection failed"))
    }

    let ok = DeferredFn::<Value>::from_fn(forty_two);
    match ok.call().await.unwrap() {
        ExpressiveEnum::Scalar(v) => assert_eq!(v, json!(42)),
        _ => panic!("expected a scalar result"),
    }

    let failing = DeferredFn::<Value>::from_fn(broken);
    let err = failing.call().await.unwrap_err();
    assert!(err.to_string().contains("API connection failed"));
}

#[tokio::test]
async fn deferred_can_return_a_nested_expression() {
    let mock = mock_builder::new()
        .with_flattening()
        .on_exact_select(
            "SELECT * FROM logs WHERE destination > now()",
            json!([{"id": 1}]),
        )
        .on_exact_select(
            "SELECT * FROM logs WHERE destination > \"1955-11-05\"",
            json!([{"id": 2}, {"id": 3}]),
        );
    let time_function = |date: Option<&'static str>| {
        DeferredFn::<Value>::new(move || {
            Box::pin(async move {
                Ok(match date {
                    Some(d) => ExpressiveEnum::Scalar(json!(d)),
                    None => ExpressiveEnum::Nested(expr!("now()")),
                })
            })
        })
    };

    let now = expr!("SELECT * FROM logs WHERE destination > {}", {
        time_function(None)
    });
    assert_eq!(
        mock.execute(&now).await.unwrap().as_array().unwrap().len(),
        1
    );

    let dated = expr!("SELECT * FROM logs WHERE destination > {}", {
        time_function(Some("1955-11-05"))
    });
    assert_eq!(
        mock.execute(&dated)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn defer_runs_the_first_query_when_the_second_executes() {
    let letters = mock_builder::new().on_exact_select(
        "SELECT date FROM letter WHERE recipient = \"Marty\"",
        json!("1885-09-02"),
    );
    let flux = mock_builder::new().with_flattening().on_exact_select(
        "CALL initiate_time_travel(\"1885-09-02\")",
        json!("Back to 1885!"),
    );

    let reference = letters.defer(expr!(
        "SELECT date FROM letter WHERE recipient = {}",
        "Marty"
    ));
    let main = expr!("CALL initiate_time_travel({})", { reference });

    assert_eq!(flux.execute(&main).await.unwrap(), "Back to 1885!");
}

#[tokio::test]
async fn closure_parameter_becomes_deferred() {
    let closure = move || -> std::pin::Pin<
        Box<dyn std::future::Future<Output = vantage_core::Result<ExpressiveEnum<Value>>> + Send>,
    > { Box::pin(async move { Ok(ExpressiveEnum::Scalar(json!(42))) }) };

    let query = expr!("SELECT * FROM test WHERE value = {}", { closure });

    let ExpressiveEnum::Deferred(deferred) = &query.parameters[0] else {
        panic!("expected a deferred parameter");
    };
    match deferred.call().await.unwrap() {
        ExpressiveEnum::Scalar(v) => assert_eq!(v, json!(42)),
        _ => panic!("expected a scalar result"),
    }
}

#[tokio::test]
async fn expressive_enum_constructors_preview_and_clone() {
    let scalar = ExpressiveEnum::Scalar(json!("a"));
    assert_eq!(scalar.preview(), "\"a\"");

    let nested = ExpressiveEnum::nested(expr!("id = {}", 1));
    assert_eq!(nested.preview(), "id = 1");

    let deferred = ExpressiveEnum::<Value>::deferred(|| {
        Box::pin(async { Ok(ExpressiveEnum::Scalar(json!(9))) })
    });
    assert_eq!(deferred.preview(), "**deferred()");

    let copy = deferred.clone();
    let ExpressiveEnum::Deferred(f) = &copy else {
        panic!("clone must stay deferred");
    };
    match f.call().await.unwrap() {
        ExpressiveEnum::Scalar(v) => assert_eq!(v, json!(9)),
        _ => panic!("expected a scalar result"),
    }

    let nested_copy = nested.clone();
    assert_eq!(nested_copy.preview(), nested.preview());
    let scalar_copy = scalar.clone();
    assert_eq!(scalar_copy.preview(), scalar.preview());
}

#[test]
fn expressive_enum_debug_names_the_variant() {
    let scalar = ExpressiveEnum::Scalar(json!(1));
    let nested = ExpressiveEnum::Nested(expr!("id = {}", 1));
    let deferred = ExpressiveEnum::<Value>::deferred(|| {
        Box::pin(async { Ok(ExpressiveEnum::Scalar(json!(1))) })
    });

    assert!(format!("{:?}", scalar).contains("Scalar"));
    assert!(format!("{:?}", nested).contains("Nested"));
    let deferred_debug = format!("{:?}", deferred);
    assert!(deferred_debug.contains("Deferred"));
    assert!(deferred_debug.contains("DeferredFn"));
}

#[test]
fn json_value_and_deferred_fn_convert_into_expressive_enum() {
    let scalar: ExpressiveEnum<Value> = json!(5).into();
    assert!(matches!(scalar, ExpressiveEnum::Scalar(_)));

    let deferred: ExpressiveEnum<Value> =
        DeferredFn::<Value>::from_mutex(Arc::new(Mutex::new(1))).into();
    assert!(matches!(deferred, ExpressiveEnum::Deferred(_)));
}

#[test]
fn scalar_is_expressive_in_its_own_type() {
    use vantage_expressions::Expressive;

    let price: Expression<i64> = 150i64.expr();
    assert_eq!(price.template, "{}");
    assert_eq!(price.preview(), "150");
}
