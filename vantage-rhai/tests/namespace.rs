use vantage_rhai::rhai::{Dynamic, EvalAltResult};
use vantage_rhai::{Block, Env, Host, Limits, Namespace, RhaiError};

fn host() -> Host {
    Host::builder(Limits::Ui)
        .vocab_fn(|engine| {
            engine.register_fn("first", |n: i64| n + 1);
            let mut ns = Namespace::new("demo");
            ns.func("first", |n: i64| n * 10)
                .func("first", |a: i64, b: i64| a * b)
                .func("half", |n: i64| -> Result<i64, Box<EvalAltResult>> {
                    if n % 2 == 0 {
                        Ok(n / 2)
                    } else {
                        Err("odd".into())
                    }
                })
                .var("answer", 42_i64)
                .var("currency", "EUR".to_string());
            ns.register(engine);
        })
        .build()
}

fn eval_on(host: &Host, code: &str) -> Result<Dynamic, RhaiError> {
    host.compile(&Block::from(code))?.eval(&Env::new())
}

fn eval(code: &str) -> Result<Dynamic, RhaiError> {
    eval_on(&host(), code)
}

fn int(code: &str) -> i64 {
    eval(code).unwrap().as_int().unwrap()
}

#[test]
fn a_namespaced_function_does_not_shadow_the_global_one_of_the_same_name() {
    assert_eq!(int("first(2)"), 3);
    assert_eq!(int("demo::first(2)"), 20);
}

#[test]
fn registering_a_name_again_with_other_parameter_types_adds_an_overload() {
    assert_eq!(int("demo::first(2)"), 20);
    assert_eq!(int("demo::first(2, 3)"), 6);
}

#[test]
fn a_namespaced_function_is_not_callable_unqualified() {
    let err = eval("half(4)").unwrap_err();
    assert!(err.to_string().contains("half"), "{err}");
}

#[test]
fn a_namespaced_function_is_not_a_method() {
    let err = eval("4.half()").unwrap_err();
    assert!(err.to_string().contains("half"), "{err}");
}

#[test]
fn a_fallible_function_returns_its_value_or_throws() {
    assert_eq!(int("demo::half(4)"), 2);
    let err = eval("demo::half(3)").unwrap_err();
    assert!(matches!(err, RhaiError::Runtime(_)), "{err:?}");
    assert!(err.to_string().contains("odd"), "{err}");
}

#[test]
fn a_fallible_function_error_can_be_caught_by_the_script() {
    assert_eq!(
        eval("let r = 0; try { r = demo::half(3); } catch (e) { r = -1; } r")
            .unwrap()
            .as_int()
            .unwrap(),
        -1
    );
}

#[test]
fn a_namespaced_constant_is_read_qualified() {
    assert_eq!(int("demo::answer + 1"), 43);
    assert_eq!(
        eval("demo::currency").unwrap().into_string().unwrap(),
        "EUR"
    );
}

#[test]
fn a_namespaced_constant_is_readable_inside_a_script_function() {
    assert_eq!(int("fn twice() { demo::answer * 2 } twice()"), 84);
}

#[test]
fn a_namespaced_constant_is_not_a_global_variable() {
    assert!(eval("answer").is_err());
}

#[test]
fn registering_a_name_again_replaces_the_earlier_namespace() {
    let host = Host::builder(Limits::Ui)
        .vocab_fn(|engine| {
            let mut first = Namespace::new("demo");
            first.func("one", || 1_i64).func("only_first", || 0_i64);
            first.register(engine);
            let mut second = Namespace::new("demo");
            second.func("one", || 2_i64);
            second.register(engine);
        })
        .build();
    assert_eq!(eval_on(&host, "demo::one()").unwrap().as_int().unwrap(), 2);
    let err = eval_on(&host, "demo::only_first()").unwrap_err();
    assert!(err.to_string().contains("only_first"), "{err}");
}

#[test]
fn namespaces_with_different_names_coexist() {
    let host = Host::builder(Limits::Ui)
        .vocab_fn(|engine| {
            let mut sql = Namespace::new("sql");
            sql.func("count", || 1_i64);
            sql.register(engine);
            let mut mongo = Namespace::new("mongo");
            mongo.func("count", || 2_i64);
            mongo.register(engine);
        })
        .build();
    assert_eq!(eval_on(&host, "sql::count()").unwrap().as_int().unwrap(), 1);
    assert_eq!(
        eval_on(&host, "mongo::count()").unwrap().as_int().unwrap(),
        2
    );
}

#[test]
fn a_namespace_registered_last_cannot_replace_a_global_verb() {
    let host = Host::builder(Limits::Ui)
        .vocab_fn(|engine| {
            engine.register_fn("table", |name: &str| format!("table:{name}"));
            let mut ns = Namespace::new("surreal");
            ns.func("table", |name: &str| format!("surreal:{name}"));
            ns.register(engine);
        })
        .build();
    assert_eq!(
        eval_on(&host, r#"table("x")"#)
            .unwrap()
            .into_string()
            .unwrap(),
        "table:x"
    );
    assert_eq!(
        eval_on(&host, r#"surreal::table("x")"#)
            .unwrap()
            .into_string()
            .unwrap(),
        "surreal:x"
    );
}

#[test]
fn an_unregistered_namespace_is_an_error() {
    assert!(eval("nosuch::thing()").is_err());
}

#[test]
fn a_float_function_computes_from_its_arguments() {
    let host = Host::builder(Limits::Ui)
        .vocab_fn(|engine| {
            let mut ns = Namespace::new("invoice");
            ns.func("total", |net: f64, rate: f64| net * (1.0 + rate))
                .var("currency", "EUR".to_string());
            ns.register(engine);
        })
        .build();
    let total = eval_on(&host, "invoice::total(100.0, 0.25)")
        .unwrap()
        .as_float()
        .unwrap();
    assert_eq!(total, 125.0);
    assert_eq!(
        eval_on(&host, "invoice::currency")
            .unwrap()
            .into_string()
            .unwrap(),
        "EUR"
    );
}
