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
fn a_namespaced_function_overloads_without_shadowing_the_global_one() {
    let got = eval("[first(2), demo::first(2), demo::first(2, 3)]")
        .unwrap()
        .into_typed_array::<i64>()
        .unwrap();
    assert_eq!(got, [3, 20, 6]);
}

#[test]
fn a_namespaced_item_is_reachable_only_qualified() {
    let function = eval("half(4)").unwrap_err();
    let constant = eval("answer").unwrap_err();
    assert!(function.to_string().contains("half"), "{function}");
    assert!(
        matches!(&constant, RhaiError::UnknownName { path, .. } if path == "answer"),
        "{constant:?}"
    );
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
fn a_namespaced_constant_is_read_qualified_even_inside_a_script_function() {
    let got = eval("fn twice() { demo::answer * 2 } demo::currency + twice()")
        .unwrap()
        .into_string()
        .unwrap();
    assert_eq!(got, "EUR84");
}

#[test]
fn a_namespace_registered_again_replaces_the_earlier_one_and_other_names_coexist() {
    let host = Host::builder(Limits::Ui)
        .vocab_fn(|engine| {
            let mut sql = Namespace::new("sql");
            sql.func("count", || 1_i64);
            sql.register(engine);
            let mut mongo = Namespace::new("mongo");
            mongo.func("count", || 2_i64);
            mongo.register(engine);
            let mut first = Namespace::new("demo");
            first.func("one", || 1_i64).func("only_first", || 0_i64);
            first.register(engine);
            let mut second = Namespace::new("demo");
            second.func("one", || 2_i64);
            second.register(engine);
        })
        .build();
    let got = eval_on(&host, "[sql::count(), mongo::count(), demo::one()]")
        .unwrap()
        .into_typed_array::<i64>()
        .unwrap();
    assert_eq!(got, [1, 2, 2]);
    let err = eval_on(&host, "demo::only_first()").unwrap_err();
    assert!(err.to_string().contains("only_first"), "{err}");
}
