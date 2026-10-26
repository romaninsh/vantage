use vantage_rhai::rhai::FnPtr;
use vantage_rhai::{Block, Env, Expr, Host, Limits, RhaiError, Template};

fn host() -> Host {
    Host::builder(Limits::Ui).build()
}

fn int(value: vantage_rhai::rhai::Dynamic) -> i64 {
    value.as_int().unwrap()
}

#[test]
fn a_compiled_slot_evaluates_against_whichever_env_it_is_given() {
    let c = host().compile(&Expr::from("qty * price")).unwrap();
    let small = Env::new().var("qty", 3_i64).var("price", 5_i64);
    let large = Env::new().var("qty", 10_i64).var("price", 2_i64);
    assert_eq!(int(c.eval(&small).unwrap()), 15);
    assert_eq!(int(c.eval(&large).unwrap()), 20);
}

#[test]
fn a_pushed_variable_can_be_a_string_a_bool_or_unit() {
    let c = host().compile(&Expr::from("name + \"!\"")).unwrap();
    let env = Env::new().var("name", "Ada");
    assert_eq!(c.eval_as::<String>(&env).unwrap(), "Ada!");

    let c = host().compile(&Expr::from("flag")).unwrap();
    assert!(c.eval_bool(&Env::new().var("flag", true)).unwrap());

    let c = host().compile(&Expr::from("nothing")).unwrap();
    assert!(c.eval(&Env::new().var("nothing", ())).unwrap().is_unit());
}

#[test]
fn compiled_keeps_the_slot_source() {
    let c = host().compile(&Expr::from("a + b")).unwrap();
    assert_eq!(c.src(), "a + b");
}

#[test]
fn an_expr_tolerates_one_wrapper_around_the_whole_string() {
    let c = host().compile(&Expr::from("${ x * 21 }")).unwrap();
    assert_eq!(int(c.eval(&Env::new().var("x", 2_i64)).unwrap()), 42);
}

#[test]
fn a_wrapped_boolean_expr_is_a_bool_not_the_text_true() {
    let c = host().compile(&Expr::from("${ row_ok }")).unwrap();
    assert!(c.eval_bool(&Env::new().var("row_ok", true)).unwrap());
}

#[test]
fn an_expr_with_two_holes_is_not_a_wrapper() {
    assert!(matches!(
        host().compile(&Expr::from("${x}${x}")),
        Err(RhaiError::Syntax(_))
    ));
}

#[test]
fn an_expr_refuses_statements() {
    assert!(matches!(
        host().compile(&Expr::from("let a = 1; a")),
        Err(RhaiError::Syntax(_))
    ));
}

#[test]
fn eval_bool_on_a_non_bool_is_wrong_type() {
    let c = host().compile(&Expr::from("1 + 1")).unwrap();
    let err = c.eval_bool(&Env::new()).unwrap_err();
    assert!(
        matches!(
            err,
            RhaiError::WrongType {
                expected: "bool",
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(err.src(), "1 + 1");
}

#[test]
fn eval_bool_returns_the_boolean() {
    let c = host().compile(&Expr::from("1 < 2")).unwrap();
    assert!(c.eval_bool(&Env::new()).unwrap());
    let c = host().compile(&Expr::from("1 > 2")).unwrap();
    assert!(!c.eval_bool(&Env::new()).unwrap());
}

#[test]
fn eval_as_casts_to_the_requested_type() {
    let c = host().compile(&Expr::from("40 + 2")).unwrap();
    assert_eq!(c.eval_as::<i64>(&Env::new()).unwrap(), 42);
    let c = host().compile(&Expr::from("1.5 + 1.0")).unwrap();
    assert_eq!(c.eval_as::<f64>(&Env::new()).unwrap(), 2.5);
}

#[test]
fn eval_as_with_the_wrong_type_is_wrong_type_not_a_panic() {
    let c = host().compile(&Expr::from("40 + 2")).unwrap();
    let err = c.eval_as::<String>(&Env::new()).unwrap_err();
    assert!(matches!(err, RhaiError::WrongType { .. }), "{err:?}");
    assert_eq!(err.src(), "40 + 2");
}

#[test]
fn a_template_without_holes_is_literal() {
    let c = host().compile(&Template::from("plain text")).unwrap();
    assert!(c.is_literal());
    assert_eq!(c.eval(&Env::new()).unwrap().to_string(), "plain text");
}

#[test]
fn a_template_with_a_hole_is_not_literal() {
    let c = host().compile(&Template::from("n=${n}")).unwrap();
    assert!(!c.is_literal());
}

#[test]
fn a_template_joins_text_and_holes_into_a_string() {
    let c = host()
        .compile(&Template::from("Order ${id} in ${cur}: ${id + 1}[${u}]"))
        .unwrap();
    let env = Env::new().var("id", 42_i64).var("cur", "USD").var("u", ());
    assert_eq!(c.eval_as::<String>(&env).unwrap(), "Order 42 in USD: 43[]");
}

#[test]
fn a_template_that_is_one_hole_keeps_the_type_of_its_value() {
    let c = host().compile(&Template::from("${ n }")).unwrap();
    let env = Env::new().var("n", 7_i64);
    assert_eq!(c.eval_as::<i64>(&env).unwrap(), 7);
}

#[test]
fn a_one_hole_template_can_yield_a_map() {
    let c = host()
        .compile(&Template::from(r#"${ #{ text: label, color: "red" } }"#))
        .unwrap();
    let env = Env::new().var("label", "Ada");
    let map = c.eval_as::<vantage_rhai::rhai::Map>(&env).unwrap();
    assert_eq!(map["text"].clone().into_string().unwrap(), "Ada");
    assert_eq!(map["color"].clone().into_string().unwrap(), "red");
}

#[test]
fn a_one_hole_template_does_not_render_to_text() {
    let c = host().compile(&Template::from("${ 1 + 1 }")).unwrap();
    let err = c.eval_as::<String>(&Env::new()).unwrap_err();
    assert!(matches!(err, RhaiError::WrongType { .. }), "{err:?}");
}

#[test]
fn visible_text_around_a_hole_turns_its_value_into_text() {
    let env = Env::new();
    let before = host().compile(&Template::from("x${ 1 + 1 }")).unwrap();
    assert_eq!(before.eval_as::<String>(&env).unwrap(), "x2");
    let after = host().compile(&Template::from("${ 1 + 1 }x")).unwrap();
    assert_eq!(after.eval_as::<String>(&env).unwrap(), "2x");
}

#[test]
fn whitespace_around_a_lone_hole_does_not_turn_its_value_into_text() {
    // A YAML block scalar leaves a newline after `${ … }`.
    let env = Env::new().var("n", 7_i64);
    let block = host().compile(&Template::from("${ n }\n")).unwrap();
    assert_eq!(block.eval_as::<i64>(&env).unwrap(), 7);
    let padded = host().compile(&Template::from("  ${ n }  \n")).unwrap();
    assert_eq!(padded.eval_as::<i64>(&env).unwrap(), 7);
    let with_text = host().compile(&Template::from(" ${ n } x")).unwrap();
    assert_eq!(with_text.eval_as::<String>(&env).unwrap(), " 7 x");
}

#[test]
fn a_block_evaluates_to_its_final_expression() {
    let c = host().compile(&Block::from("let a = 40; a + 2")).unwrap();
    assert_eq!(int(c.eval(&Env::new()).unwrap()), 42);
    assert_eq!(c.eval_as::<i64>(&Env::new()).unwrap(), 42);
}

#[test]
fn a_block_ending_on_a_statement_evaluates_to_unit() {
    let c = host().compile(&Block::from("let a = 1;")).unwrap();
    assert!(c.eval(&Env::new()).unwrap().is_unit());
}

#[test]
fn run_executes_a_block_for_effect() {
    let c = host().compile(&Block::from("let a = 40; a + 2")).unwrap();
    assert!(c.run(&Env::new()).is_ok());
}

#[test]
fn a_throw_is_a_runtime_error_carrying_its_message() {
    let top = host().compile(&Block::from(r#"throw "boom""#)).unwrap();
    let in_fn = host()
        .compile(&Block::from(r#"fn f() { throw "bang" } f()"#))
        .unwrap();
    let top = top.run(&Env::new()).unwrap_err();
    let in_fn = in_fn.run(&Env::new()).unwrap_err();
    assert!(matches!(top, RhaiError::Runtime(_)), "{top:?}");
    assert!(top.to_string().contains("boom"), "{top}");
    assert!(matches!(in_fn, RhaiError::Runtime(_)), "{in_fn:?}");
    assert!(in_fn.to_string().contains("bang"), "{in_fn}");
}

#[test]
fn a_block_syntax_error_fails_at_compile() {
    let err = host().compile(&Block::from("let a = ;")).unwrap_err();
    assert!(matches!(err, RhaiError::Syntax(_)), "{err:?}");
    assert_eq!(err.src(), "let a = ;");
}

#[test]
fn a_template_hole_syntax_error_fails_at_compile() {
    let err = host().compile(&Template::from("a ${ 1 + } b")).unwrap_err();
    assert!(matches!(err, RhaiError::Syntax(_)), "{err:?}");
}

#[test]
fn an_unterminated_hole_fails_at_compile_with_a_position() {
    let err = host().compile(&Template::from("a\nbb ${x")).unwrap_err();
    let RhaiError::Syntax(located) = &err else {
        panic!("expected Syntax, got {err:?}")
    };
    assert_eq!((located.line, located.column), (2, 4));
    assert!(
        located.message.contains("unterminated"),
        "{}",
        located.message
    );
    assert_eq!(err.src(), "a\nbb ${x");
}

#[test]
fn an_unterminated_hole_column_counts_characters_not_bytes() {
    // "café " is five characters; the caret belongs under the `$`.
    let err = host().compile(&Template::from("café ${x")).unwrap_err();
    let RhaiError::Syntax(located) = err else {
        panic!("expected Syntax")
    };
    assert_eq!((located.line, located.column), (1, 6));
}

#[test]
fn a_hole_runtime_error_reports_template_coordinates() {
    let c = host()
        .compile(&Template::from(
            "short\nthis is a longer line ${ nosuchfn(1) }",
        ))
        .unwrap();
    let err = c.eval(&Env::new()).unwrap_err();
    let RhaiError::Runtime(located) = &err else {
        panic!("expected Runtime, got {err:?}")
    };
    assert_eq!(located.line, 2, "line indexes the template, not the hole");
    assert_eq!(
        located.column, 26,
        "column is offset by where the hole starts"
    );
    assert_eq!(err.src(), "short\nthis is a longer line ${ nosuchfn(1) }");
    let shown = located.to_string();
    assert!(shown.contains("this is a longer line"), "{shown}");
    assert_eq!(
        shown.matches("line 2").count(),
        1,
        "position printed once: {shown}"
    );
}

#[test]
fn a_hole_syntax_error_reports_template_coordinates() {
    let template = "line one\nvalue is ${ 1 + } here";
    let err = host().compile(&Template::from(template)).unwrap_err();
    let RhaiError::Syntax(located) = &err else {
        panic!("expected Syntax, got {err:?}")
    };
    assert_eq!(located.line, 2, "line indexes the template");
    assert!(
        located.column >= 13,
        "column sits inside the hole, got {}",
        located.column
    );
    assert_eq!(located.src, template);
    assert_eq!(err.src(), template);
}

#[test]
fn a_syntax_error_displays_the_line_and_a_caret() {
    let err = host()
        .compile(&Block::from("let a = 1;\nlet b = a +;"))
        .unwrap_err();
    let shown = err.to_string();
    assert!(shown.starts_with("Syntax error:"), "{shown}");
    assert!(shown.contains("line 2"), "{shown}");
    assert!(shown.contains("| let b = a +;"), "{shown}");
    assert!(
        shown.lines().last().unwrap().trim_end().ends_with('^'),
        "{shown}"
    );
}

#[test]
fn a_runaway_loop_is_a_limit_error_naming_operations() {
    let c = host().compile(&Block::from("while true {}")).unwrap();
    let err = c.run(&Env::new()).unwrap_err();
    assert!(
        matches!(
            err,
            RhaiError::LimitExceeded {
                limit: "operations",
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(err.src(), "while true {}");
}

#[test]
fn compile_uncached_evaluates_like_compile() {
    let h = host();
    let slot = Expr::from("x * 2");
    let cached = h.compile(&slot).unwrap();
    let uncached = h.compile_uncached(&slot).unwrap();
    let env = Env::new().var("x", 21_i64);
    assert_eq!(
        int(cached.eval(&env).unwrap()),
        int(uncached.eval(&env).unwrap())
    );
    assert_eq!(uncached.src(), "x * 2");
}

#[test]
fn compile_uncached_reports_syntax_errors() {
    assert!(matches!(
        host().compile_uncached(&Block::from("let = 1;")),
        Err(RhaiError::Syntax(_))
    ));
}

#[test]
fn ast_is_available_for_an_expr_a_block_and_a_lone_hole() {
    let h = host();
    assert!(h.compile(&Expr::from("1 + 1")).unwrap().ast().is_some());
    assert!(
        h.compile(&Block::from("let a = 1; a"))
            .unwrap()
            .ast()
            .is_some()
    );
    assert!(
        h.compile(&Template::from("${ 1 + 1 }"))
            .unwrap()
            .ast()
            .is_some()
    );
}

#[test]
fn ast_is_none_for_a_template_with_text_around_its_holes() {
    let h = host();
    assert!(
        h.compile(&Template::from("n=${ 1 }"))
            .unwrap()
            .ast()
            .is_none()
    );
    assert!(h.compile(&Template::from("plain")).unwrap().ast().is_none());
    assert!(
        h.compile(&Template::from("${ 1 }${ 2 }"))
            .unwrap()
            .ast()
            .is_none()
    );
}

#[test]
fn engine_and_ast_let_a_caller_invoke_a_closure_the_script_returned() {
    let c = host().compile(&Expr::from("|x| x + 1")).unwrap();
    let ptr = c.eval_as::<FnPtr>(&Env::new()).unwrap();
    let out: i64 = ptr.call(c.engine(), c.ast().unwrap(), (41_i64,)).unwrap();
    assert_eq!(out, 42);
}

#[test]
fn compiled_debug_output_names_the_source() {
    let c = host().compile(&Expr::from("a + b")).unwrap();
    let shown = format!("{c:?}");
    assert!(shown.contains("a + b"), "{shown}");
}
