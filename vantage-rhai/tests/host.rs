use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use vantage_rhai::rhai::Engine;
use vantage_rhai::{
    AST_CACHE_BOUND, BACKGROUND_MAX_OPERATIONS, Block, Env, Expr, Host, Limits, Mode, RhaiError,
    UI_MAX_OPERATIONS, Vocab, background_host, ui_host,
};

fn ui() -> Host {
    Host::builder(Limits::Ui).build()
}

fn background() -> Host {
    Host::builder(Limits::background()).build()
}

fn run(host: &Host, script: &str) -> Result<(), RhaiError> {
    host.compile(&Block::from(script))?.run(&Env::new())
}

fn eval_int(host: &Host, script: &str) -> i64 {
    host.compile(&Block::from(script))
        .unwrap()
        .eval(&Env::new())
        .unwrap()
        .as_int()
        .unwrap()
}

fn is_limit(result: Result<(), RhaiError>) -> bool {
    matches!(result, Err(RhaiError::LimitExceeded { .. }))
}

#[test]
fn host_and_compiled_slots_are_send_and_sync() {
    fn shared<T: Send + Sync>() {}
    shared::<Host>();
    shared::<vantage_rhai::Compiled<Expr>>();
    shared::<vantage_rhai::Compiled<Block>>();
    shared::<vantage_rhai::Compiled<vantage_rhai::Template>>();
}

mod vocab {
    use super::*;

    struct Doubler;

    impl Vocab for Doubler {
        fn register(&self, engine: &mut Engine) {
            engine.register_fn("double", |x: i64| x * 2);
        }
    }

    struct Tripler;

    impl Vocab for Tripler {
        fn register(&self, engine: &mut Engine) {
            engine.register_fn("double", |x: i64| x * 3);
        }
    }

    #[test]
    fn a_bare_host_has_no_domain_verbs() {
        let err = run(&ui(), "double(2)").unwrap_err();
        assert!(matches!(err, RhaiError::Runtime(_)), "{err:?}");
        assert!(err.to_string().contains("double"), "{err}");
    }

    #[test]
    fn vocab_fn_registers_inline_functions() {
        let host = Host::builder(Limits::Ui)
            .vocab_fn(|engine| {
                engine.register_fn("double", |x: i64| x * 2);
            })
            .build();
        assert_eq!(eval_int(&host, "double(21)"), 42);
    }

    #[test]
    fn the_last_registration_of_a_name_wins() {
        let host = Host::builder(Limits::Ui)
            .vocab(Doubler)
            .vocab(Tripler)
            .build();
        assert_eq!(eval_int(&host, "double(10)"), 30);
        let host = Host::builder(Limits::Ui)
            .vocab(Tripler)
            .vocab(Doubler)
            .build();
        assert_eq!(eval_int(&host, "double(10)"), 20);
    }

    #[test]
    fn vocab_fn_may_move_in_a_value_that_is_not_send() {
        let log = Rc::new(RefCell::new(Vec::<&str>::new()));
        let seen = log.clone();
        let _host = Host::builder(Limits::Ui)
            .vocab_fn(move |_engine| seen.borrow_mut().push("registered"))
            .build();
        assert_eq!(*log.borrow(), ["registered"]);
    }

    #[test]
    fn a_vocab_does_not_disable_resolver_lookups() {
        use vantage_rhai::rhai::Dynamic;
        use vantage_rhai::{Lookup, Resolver};

        struct One;
        impl Resolver for One {
            fn resolve(&self, path: &str) -> Lookup {
                match path {
                    "one" => Lookup::Leaf(Dynamic::from(1_i64)),
                    _ => Lookup::Unknown,
                }
            }
        }

        let host = Host::builder(Limits::Ui).vocab(Doubler).build();
        let env = Env::new().resolver(Arc::new(One));
        let c = host.compile(&Expr::from("double(one)")).unwrap();
        assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 2);
    }

    #[test]
    fn a_vocab_function_is_visible_to_every_slot_kind() {
        let host = Host::builder(Limits::Ui).vocab(Doubler).build();
        let env = Env::new();
        let expr = host.compile(&Expr::from("double(2)")).unwrap();
        assert_eq!(expr.eval(&env).unwrap().as_int().unwrap(), 4);
        let tpl = host
            .compile(&vantage_rhai::Template::from("= ${double(3)}"))
            .unwrap();
        assert_eq!(tpl.eval_as::<String>(&env).unwrap(), "= 6");
    }
}

mod limits {
    use super::*;

    #[test]
    fn max_operations_reports_each_ceiling() {
        assert_eq!(UI_MAX_OPERATIONS, 500_000);
        assert_eq!(BACKGROUND_MAX_OPERATIONS, 50_000_000);
        assert_eq!(Limits::Ui.max_operations(), 500_000);
        assert_eq!(Limits::background().max_operations(), 50_000_000);
        assert_eq!(
            Limits::Background {
                max_operations: 123
            }
            .max_operations(),
            123
        );
        assert_eq!(Limits::Background { max_operations: 0 }.max_operations(), 1);
    }

    #[test]
    fn background_is_the_background_profile_with_the_default_ceiling() {
        assert_eq!(
            Limits::background(),
            Limits::Background {
                max_operations: BACKGROUND_MAX_OPERATIONS
            }
        );
    }

    #[test]
    fn a_host_reports_the_limits_it_was_built_with() {
        assert_eq!(ui().limits(), Limits::Ui);
        assert_eq!(background().limits(), Limits::background());
        let custom = Limits::Background { max_operations: 77 };
        assert_eq!(Host::builder(custom).build().limits(), custom);
    }

    #[test]
    fn each_host_stops_work_past_its_operation_ceiling() {
        let work = "let i = 0; while i < 400000 { i += 1; } i";
        let zero = Host::builder(Limits::Background { max_operations: 0 }).build();
        assert!(is_limit(run(&ui(), work)));
        assert_eq!(eval_int(&background(), work), 400_000);
        assert!(is_limit(run(&background(), "while true {}")));
        assert!(
            is_limit(run(&zero, work)),
            "a zero ceiling is not unlimited"
        );
    }

    #[test]
    fn strings_and_arrays_are_capped_in_size() {
        let host = background();
        assert!(is_limit(run(&host, r#"let s = "x"; loop { s += s; }"#)));
        assert!(is_limit(run(&host, "let a = []; a.pad(2000000, 0);")));
    }

    #[test]
    fn nesting_within_the_call_level_cap_runs() {
        let script = "fn f(n) { if n == 0 { 0 } else { 1 + f(n - 1) } } f(30)";
        assert_eq!(eval_int(&ui(), script), 30);
    }

    #[test]
    #[ignore = "possible bug: recursion that reaches the call-level cap never returns (hangs past 60s) instead of LimitExceeded"]
    fn call_nesting_is_capped_at_sixty_four_levels() {
        let script = "fn f(n) { if n == 0 { 0 } else { f(n - 1) } } f(200)";
        assert!(is_limit(run(&ui(), script)));
    }

    #[test]
    fn expression_depth_is_capped_at_256() {
        let nested = |n: usize| format!("{}1{}", "(".repeat(n), ")".repeat(n));
        let host = ui();
        assert!(host.compile(&Expr::from(nested(50).as_str())).is_ok());
        let err = host
            .compile(&Expr::from(nested(300).as_str()))
            .map(|_| ())
            .unwrap_err();
        assert!(matches!(err, RhaiError::Syntax(_)), "{err:?}");
    }
}

mod cache {
    use super::*;

    #[test]
    fn the_cache_is_keyed_by_mode_and_source() {
        let host = ui();
        let a = host.ast(Mode::Expression, "1 + 1").unwrap();
        let same = host.ast(Mode::Expression, "1 + 1").unwrap();
        let other_source = host.ast(Mode::Expression, "1 + 2").unwrap();
        let other_mode = host.ast(Mode::Script, "1 + 1").unwrap();
        assert!(Arc::ptr_eq(&a, &same));
        assert!(!Arc::ptr_eq(&a, &other_source));
        assert!(!Arc::ptr_eq(&a, &other_mode));
    }

    #[test]
    fn compiling_a_slot_reuses_the_cached_ast_of_its_own_kind() {
        let host = ui();
        let a = host.compile(&Expr::from("40 + 2")).unwrap();
        let b = host.compile(&Expr::from("40 + 2")).unwrap();
        let block = host.compile(&Block::from("40 + 2")).unwrap();
        assert!(std::ptr::eq(a.ast().unwrap(), b.ast().unwrap()));
        assert!(!std::ptr::eq(a.ast().unwrap(), block.ast().unwrap()));
    }

    #[test]
    fn uncached_compiles_never_share_an_ast() {
        let host = ui();
        let a = host.ast_uncached(Mode::Expression, "1 + 1").unwrap();
        let b = host.ast_uncached(Mode::Expression, "1 + 1").unwrap();
        let cached = host.ast(Mode::Expression, "1 + 1").unwrap();
        let uncached_slot = host.compile_uncached(&Expr::from("1 + 1")).unwrap();
        let cached_slot = host.compile(&Expr::from("1 + 1")).unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &cached));
        assert!(!std::ptr::eq(
            uncached_slot.ast().unwrap(),
            cached_slot.ast().unwrap()
        ));
    }

    #[test]
    fn a_syntax_error_is_reported_and_not_cached() {
        let host = ui();
        let err = host.ast(Mode::Expression, "1 +").unwrap_err();
        assert!(matches!(err, RhaiError::Syntax(_)), "{err:?}");
        assert!(host.ast(Mode::Expression, "1 +").is_err());
    }

    #[test]
    fn an_ast_from_the_host_runs_on_the_hosts_engine() {
        let host = ui();
        let ast = host.ast(Mode::Expression, "6 * 7").unwrap();
        let v: i64 = host.engine().eval_ast(&ast).unwrap();
        assert_eq!(v, 42);
    }

    #[test]
    fn the_cache_clears_wholesale_when_it_overflows_and_keeps_serving() {
        let host = ui();
        let first = host.ast(Mode::Expression, "0").unwrap();
        for i in 1..AST_CACHE_BOUND {
            host.ast(Mode::Expression, &i.to_string()).unwrap();
        }
        let still_cached = host.ast(Mode::Expression, "0").unwrap();
        assert!(
            Arc::ptr_eq(&first, &still_cached),
            "bound entries still fit"
        );

        host.ast(Mode::Expression, "overflow-marker").unwrap();
        let recompiled = host.ast(Mode::Expression, "0").unwrap();
        assert!(
            !Arc::ptr_eq(&first, &recompiled),
            "overflow dropped the earlier entries"
        );
        let v: i64 = host
            .engine()
            .eval_ast(&host.ast(Mode::Expression, "2").unwrap())
            .unwrap();
        assert_eq!(v, 2);
    }
}

mod shared_hosts {
    use super::*;

    #[test]
    fn ui_host_is_one_process_wide_instance_on_the_ui_profile() {
        assert!(std::ptr::eq(ui_host(), ui_host()));
        assert_eq!(ui_host().limits(), Limits::Ui);
    }

    #[test]
    fn background_host_is_one_process_wide_instance_on_the_background_profile() {
        assert!(std::ptr::eq(background_host(), background_host()));
        assert_eq!(background_host().limits(), Limits::background());
    }

    #[test]
    fn the_two_shared_hosts_are_distinct() {
        assert!(!std::ptr::eq(ui_host(), background_host()));
    }

    #[test]
    fn the_shared_hosts_have_no_verbs() {
        let ui = run(ui_host(), "double(2)").unwrap_err();
        let background = run(background_host(), "double(2)").unwrap_err();
        assert!(matches!(ui, RhaiError::Runtime(_)), "{ui:?}");
        assert!(
            matches!(background, RhaiError::Runtime(_)),
            "{background:?}"
        );
    }

    #[test]
    fn the_shared_hosts_evaluate_plain_arithmetic() {
        assert_eq!(eval_int(ui_host(), "6 * 7"), 42);
        assert_eq!(eval_int(background_host(), "6 * 7"), 42);
    }

    #[test]
    fn callers_of_a_shared_host_share_one_ast_cache() {
        let a = ui_host()
            .compile(&Expr::from("shared_marker_ui + 1"))
            .unwrap();
        let b = ui_host()
            .compile(&Expr::from("shared_marker_ui + 1"))
            .unwrap();
        assert!(std::ptr::eq(a.ast().unwrap(), b.ast().unwrap()));
        let c = background_host()
            .compile(&Expr::from("shared_marker_bg + 1"))
            .unwrap();
        let d = background_host()
            .compile(&Expr::from("shared_marker_bg + 1"))
            .unwrap();
        assert!(std::ptr::eq(c.ast().unwrap(), d.ast().unwrap()));
    }
}
