use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Block, Env, Expr, Host, Limits, Lookup, Resolver, RhaiError, Template};

/// A resolver over a flat `path -> value` map. A strict prefix of a known path
/// is a namespace; anything else is unknown.
struct MapResolver(HashMap<String, Dynamic>);

impl Resolver for MapResolver {
    fn resolve(&self, path: &str) -> Lookup {
        if let Some(v) = self.0.get(path) {
            return Lookup::Leaf(v.clone());
        }
        let prefix = format!("{path}.");
        if self.0.keys().any(|k| k.starts_with(&prefix)) {
            Lookup::Namespace
        } else {
            Lookup::Unknown
        }
    }
}

fn resolver() -> Arc<dyn Resolver> {
    let mut m = HashMap::new();
    m.insert("app.toolbar.currency".to_string(), Dynamic::from("USD"));
    m.insert("app.show".to_string(), Dynamic::from(true));
    m.insert("args.id".to_string(), Dynamic::from(42_i64));
    Arc::new(MapResolver(m))
}

fn host() -> Host {
    Host::builder(Limits::Ui).build()
}

fn reads(c: &vantage_rhai::Compiled<impl vantage_rhai::Slot>) -> Vec<String> {
    c.read_set().iter().cloned().collect()
}

#[test]
fn dotted_paths_descend_through_namespaces() {
    let env = Env::new().resolver(resolver());
    let c = host().compile(&Expr::from("app.toolbar.currency")).unwrap();
    assert_eq!(c.eval_as::<String>(&env).unwrap(), "USD");
}

#[test]
fn a_resolver_can_serve_a_leaf_to_a_boolean_slot() {
    let env = Env::new().resolver(resolver());
    let c = host().compile(&Expr::from("app.show")).unwrap();
    assert!(c.eval_bool(&env).unwrap());
}

#[test]
fn an_unknown_name_is_an_error_not_unit() {
    let env = Env::new().resolver(resolver());
    let c = host().compile(&Expr::from("nothing")).unwrap();
    let err = c.eval(&env).unwrap_err();
    assert!(
        matches!(&err, RhaiError::UnknownName { path, .. } if path == "nothing"),
        "{err:?}"
    );
    assert_eq!(err.src(), "nothing");
}

#[test]
fn an_unknown_leaf_under_a_namespace_names_the_full_path() {
    let env = Env::new().resolver(resolver());
    let c = host().compile(&Expr::from("app.toolbar.nope")).unwrap();
    let err = c.eval(&env).unwrap_err();
    assert!(
        matches!(&err, RhaiError::UnknownName { path, .. } if path == "app.toolbar.nope"),
        "{err:?}"
    );
}

#[test]
fn an_unknown_name_without_any_resolver_is_unknown_name() {
    let c = host().compile(&Expr::from("nothing + 1")).unwrap();
    let err = c.eval(&Env::new()).unwrap_err();
    assert!(matches!(err, RhaiError::UnknownName { .. }), "{err:?}");
}

#[test]
fn an_unknown_name_inside_a_script_function_is_still_unknown_name() {
    let env = Env::new().resolver(resolver());
    let c = host()
        .compile(&Block::from("fn f() { app.nope } f()"))
        .unwrap();
    let err = c.eval(&env).unwrap_err();
    assert!(matches!(err, RhaiError::UnknownName { .. }), "{err:?}");
}

#[test]
fn pushed_variables_shadow_the_resolver() {
    let env = Env::new().resolver(resolver()).var("args", 7_i64);
    let c = host().compile(&Expr::from("args")).unwrap();
    assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 7);
}

#[test]
fn the_resolver_is_asked_once_per_read_on_every_evaluation() {
    struct Counting(AtomicUsize);
    impl Resolver for Counting {
        fn resolve(&self, path: &str) -> Lookup {
            let n = self.0.fetch_add(1, Ordering::SeqCst);
            match path {
                "v" => Lookup::Leaf(Dynamic::from(n as i64)),
                _ => Lookup::Unknown,
            }
        }
    }
    let counter = Arc::new(Counting(AtomicUsize::new(0)));
    let env = Env::new().resolver(counter.clone());
    let c = host().compile(&Expr::from("v")).unwrap();
    assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 0);
    assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 1);
    assert_eq!(counter.0.load(Ordering::SeqCst), 2);
}

#[test]
fn a_slot_is_not_discovered_until_discover_runs() {
    let c = host().compile(&Expr::from("app.show")).unwrap();
    assert!(!c.is_discovered());
    assert!(c.read_set().is_empty());
}

#[test]
fn discover_records_the_leaf_paths_a_slot_reads() {
    let env = Env::new().resolver(resolver());
    let c = host()
        .compile(&Expr::from("app.toolbar.currency"))
        .unwrap()
        .discover(&env)
        .unwrap();
    assert!(c.is_discovered());
    assert_eq!(reads(&c), ["app.toolbar.currency"]);
}

#[test]
fn a_discovered_slot_still_evaluates() {
    let env = Env::new().resolver(resolver());
    let c = host()
        .compile(&Expr::from("app.toolbar.currency"))
        .unwrap()
        .discover(&env)
        .unwrap();
    assert_eq!(c.eval_as::<String>(&env).unwrap(), "USD");
}

#[test]
fn discovering_a_template_unions_the_reads_of_its_holes() {
    let env = Env::new().resolver(resolver());
    let c = host()
        .compile(&Template::from(
            "Order ${args.id} in ${app.toolbar.currency}",
        ))
        .unwrap()
        .discover(&env)
        .unwrap();
    assert_eq!(reads(&c), ["app.toolbar.currency", "args.id"]);
    assert_eq!(c.eval_as::<String>(&env).unwrap(), "Order 42 in USD");
}

#[test]
fn discovering_a_block_records_reads_inside_its_statements() {
    let env = Env::new().resolver(resolver());
    let c = host()
        .compile(&Block::from("let x = args.id; x * 2"))
        .unwrap()
        .discover(&env)
        .unwrap();
    assert_eq!(reads(&c), ["args.id"]);
}

#[test]
fn an_unknown_name_fails_at_discover_not_at_compile() {
    let env = Env::new().resolver(resolver());
    let c = host()
        .compile(&Expr::from("app.toolbar.currencyy"))
        .unwrap();
    let err = c.discover(&env).map(|_| ()).unwrap_err();
    assert!(
        matches!(&err, RhaiError::UnknownName { path, .. } if path == "app.toolbar.currencyy"),
        "{err:?}"
    );
}

#[test]
fn a_pushed_variable_is_not_a_read() {
    let env = Env::new().resolver(resolver()).var("args", 1_i64);
    let c = host()
        .compile(&Expr::from("args"))
        .unwrap()
        .discover(&env)
        .unwrap();
    assert!(c.read_set().is_empty());
    assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 1);
}

#[test]
fn an_env_without_a_resolver_discovers_an_empty_set() {
    let env = Env::new().var("x", 1_i64);
    let c = host()
        .compile(&Expr::from("x + 1"))
        .unwrap()
        .discover(&env)
        .unwrap();
    assert!(
        c.is_discovered(),
        "a frozen empty set is not a never-taken one"
    );
    assert!(c.read_set().is_empty());
}

#[test]
fn discover_value_returns_the_first_evaluations_value() {
    let env = Env::new().resolver(resolver());
    let (c, v) = host()
        .compile(&Block::from("let x = args.id; x * 2"))
        .unwrap()
        .discover_value(&env)
        .unwrap();
    assert_eq!(v.as_int().unwrap(), 84);
    assert_eq!(reads(&c), ["args.id"]);
    assert!(c.is_discovered());
}

#[test]
fn a_branch_that_was_not_taken_records_no_reads() {
    let env = Env::new().resolver(resolver());
    let c = host()
        .compile(&Expr::from("if app.show { 1 } else { args.id }"))
        .unwrap()
        .discover(&env)
        .unwrap();
    assert_eq!(reads(&c), ["app.show"]);
}

#[test]
fn a_resolver_can_be_shared_between_envs() {
    let r = resolver();
    let a = Env::new().resolver(r.clone());
    let b = Env::new().resolver(r);
    let c = host().compile(&Expr::from("args.id")).unwrap();
    assert_eq!(c.eval(&a).unwrap().as_int().unwrap(), 42);
    assert_eq!(c.eval(&b).unwrap().as_int().unwrap(), 42);
}

#[test]
fn an_env_clone_keeps_its_variables_and_resolver() {
    let env = Env::new().resolver(resolver()).var("k", 2_i64);
    let copy = env.clone();
    let c = host().compile(&Expr::from("args.id * k")).unwrap();
    assert_eq!(c.eval(&copy).unwrap().as_int().unwrap(), 84);
}
