use vantage_rhai::rhai::{Dynamic, Engine, Position};
use vantage_rhai::{Located, RhaiError};

fn located(src: &str, line: usize, column: usize, message: &str) -> Located {
    Located {
        src: src.to_string(),
        line,
        column,
        message: message.to_string(),
    }
}

#[test]
fn display_draws_the_offending_line_with_a_caret_under_the_column() {
    let l = Located::new(
        "let a = 1;\nlet m = 2;",
        Position::new(2, 5),
        "boom".to_string(),
    );
    assert_eq!(
        l.to_string(),
        "boom (line 2, column 5)\n  | let m = 2;\n  |     ^"
    );
}

#[test]
fn a_caret_in_column_one_has_no_indent() {
    let l = located("let m = 2;", 1, 1, "oops");
    assert_eq!(
        l.to_string(),
        "oops (line 1, column 1)\n  | let m = 2;\n  | ^"
    );
}

#[test]
fn display_without_a_position_prints_the_message_alone() {
    let l = Located::new("let a = 1;", Position::NONE, "no position".to_string());
    assert_eq!((l.line, l.column), (0, 0));
    assert_eq!(l.to_string(), "no position");
}

#[test]
fn located_new_keeps_source_line_column_and_message() {
    let l = Located::new("a\nb", Position::new(2, 1), "msg".to_string());
    assert_eq!(l.src, "a\nb");
    assert_eq!((l.line, l.column), (2, 1));
    assert_eq!(l.message, "msg");
}

#[test]
fn at_offsets_the_column_on_the_fragments_first_line() {
    let l = Located::at(
        "ab ${ x }",
        Position::new(1, 3),
        "m".to_string(),
        Some((2, 10)),
    );
    assert_eq!((l.line, l.column), (2, 12));
}

#[test]
fn at_shifts_only_the_line_after_the_fragments_first_line() {
    let l = Located::at("src", Position::new(2, 4), "m".to_string(), Some((3, 10)));
    assert_eq!((l.line, l.column), (4, 4));
}

#[test]
fn at_without_a_remap_keeps_the_reported_position() {
    let l = Located::at("src", Position::new(2, 4), "m".to_string(), None);
    assert_eq!((l.line, l.column), (2, 4));
}

#[test]
fn at_leaves_an_unpositioned_error_unpositioned_even_with_a_remap() {
    let l = Located::at("src", Position::NONE, "m".to_string(), Some((3, 10)));
    assert_eq!((l.line, l.column), (0, 0));
}

#[test]
fn from_parse_is_a_syntax_error_with_rhais_prefix_and_location() {
    let src = "let a = 1;\nlet b = a +;";
    let parse_err = Engine::new().compile(src).unwrap_err();
    let e = RhaiError::from_parse(src, parse_err);
    let RhaiError::Syntax(l) = &e else {
        panic!("expected Syntax, got {e:?}")
    };
    assert_eq!(l.line, 2);
    assert_eq!(e.src(), src);
    let shown = e.to_string();
    assert!(shown.starts_with("Syntax error:"), "{shown}");
    assert!(shown.contains("| let b = a +;"), "{shown}");
    assert!(
        shown.lines().last().unwrap().trim_end().ends_with('^'),
        "{shown}"
    );
}

#[test]
fn from_eval_maps_too_many_operations_to_limit_exceeded() {
    let mut engine = Engine::new();
    engine.set_max_operations(1_000);
    let src = "while true {}";
    let e = RhaiError::from_eval(src, engine.run(src).unwrap_err());
    assert!(
        matches!(
            e,
            RhaiError::LimitExceeded {
                limit: "operations",
                ..
            }
        ),
        "{e:?}"
    );
    assert_eq!(e.src(), src);
}

#[test]
fn from_eval_maps_oversized_data_to_limit_exceeded() {
    let mut engine = Engine::new();
    engine.set_max_string_size(64);
    let src = r#"let s = "x"; loop { s += s; }"#;
    let e = RhaiError::from_eval(src, engine.run(src).unwrap_err());
    assert!(matches!(e, RhaiError::LimitExceeded { .. }), "{e:?}");
}

#[test]
fn from_eval_maps_runaway_recursion_to_limit_exceeded() {
    let mut engine = Engine::new();
    engine.set_max_call_levels(8);
    let src = "fn f(n) { f(n + 1) } f(0)";
    let e = RhaiError::from_eval(src, engine.run(src).unwrap_err());
    assert!(matches!(e, RhaiError::LimitExceeded { .. }), "{e:?}");
}

#[test]
fn from_eval_maps_a_missing_variable_to_unknown_name() {
    let src = "nope + 1";
    let err = Engine::new().eval::<Dynamic>(src).unwrap_err();
    let e = RhaiError::from_eval(src, err);
    assert!(
        matches!(&e, RhaiError::UnknownName { path, .. } if path == "nope"),
        "{e:?}"
    );
    assert_eq!(e.src(), src);
}

#[test]
fn from_eval_maps_a_throw_to_runtime_carrying_the_message_and_source() {
    let src = "throw \"boom\"";
    let e = RhaiError::from_eval(src, Engine::new().run(src).unwrap_err());
    assert!(matches!(e, RhaiError::Runtime(_)), "{e:?}");
    assert_eq!(e.src(), src);
    assert!(e.to_string().contains("boom"), "{e}");
}

#[test]
fn from_eval_at_moves_the_position_into_the_enclosing_text() {
    let src = "first line\nsecond ${ throw \"boom\" }";
    let err = Engine::new().run("throw \"boom\"").unwrap_err();
    let e = RhaiError::from_eval_at(src, err, Some((2, 10)));
    let RhaiError::Runtime(l) = &e else {
        panic!("expected Runtime, got {e:?}")
    };
    assert_eq!(l.line, 2);
    assert_eq!(l.column, 10, "the throw begins at column 1 of the hole");
    assert_eq!(l.src, src);
}

#[test]
fn rebase_moves_a_first_line_position_by_the_holes_start() {
    let hole_err = RhaiError::Runtime(located("1 +", 1, 3, "m"));
    let template = "ab\nsome text ${ 1 + }";
    let RhaiError::Runtime(l) = hole_err.rebase(template, (2, 15)) else {
        panic!("variant changed")
    };
    assert_eq!((l.line, l.column), (2, 17));
    assert_eq!(l.src, template);
    assert_eq!(l.message, "m");
}

#[test]
fn rebase_shifts_only_the_line_for_later_lines_of_the_hole() {
    let hole_err = RhaiError::Syntax(located("a\nb", 2, 4, "m"));
    let RhaiError::Syntax(l) = hole_err.rebase("x\ny\n${ a\nb }", (3, 4)) else {
        panic!("variant changed")
    };
    assert_eq!((l.line, l.column), (4, 4));
}

#[test]
fn rebase_keeps_an_unpositioned_error_unpositioned() {
    let hole_err = RhaiError::Runtime(located("x", 0, 0, "m"));
    let RhaiError::Runtime(l) = hole_err.rebase("outer", (2, 3)) else {
        panic!("variant changed")
    };
    assert_eq!((l.line, l.column), (0, 0));
}

#[test]
fn rebase_replaces_the_source_of_every_variant() {
    let variants = [
        RhaiError::UnknownName {
            path: "p".into(),
            src: "inner".into(),
        },
        RhaiError::WrongType {
            expected: "bool",
            actual: "i64".into(),
            src: "inner".into(),
        },
        RhaiError::LimitExceeded {
            limit: "operations",
            src: "inner".into(),
        },
        RhaiError::Syntax(located("inner", 1, 1, "m")),
        RhaiError::Runtime(located("inner", 1, 1, "m")),
    ];
    for e in variants {
        assert_eq!(e.rebase("outer text", (1, 1)).src(), "outer text");
    }
}

#[test]
fn rebase_keeps_the_details_of_non_located_variants() {
    let e = RhaiError::UnknownName {
        path: "a.b".into(),
        src: "inner".into(),
    }
    .rebase("outer", (1, 1));
    assert!(matches!(&e, RhaiError::UnknownName { path, .. } if path == "a.b"));
    let e = RhaiError::LimitExceeded {
        limit: "size",
        src: "inner".into(),
    }
    .rebase("outer", (1, 1));
    assert!(matches!(e, RhaiError::LimitExceeded { limit: "size", .. }));
}

#[test]
fn wrong_type_names_what_was_expected_and_what_arrived() {
    let e = RhaiError::wrong_type("1 + 1", "bool", &Dynamic::from(2_i64));
    assert_eq!(e.to_string(), "expected bool, got i64");
    assert_eq!(e.src(), "1 + 1");
    assert!(
        matches!(e, RhaiError::WrongType { expected: "bool", ref actual, .. } if actual == "i64")
    );
}

#[test]
fn each_variant_displays_what_went_wrong() {
    let unknown = RhaiError::UnknownName {
        path: "app.x".into(),
        src: "s".into(),
    };
    assert_eq!(unknown.to_string(), "unknown name `app.x` in scope");
    let limit = RhaiError::LimitExceeded {
        limit: "operations",
        src: "s".into(),
    };
    assert_eq!(limit.to_string(), "script exceeded its operations limit");
}

#[test]
fn every_variant_reports_its_slot_source() {
    let all = [
        RhaiError::Syntax(located("a", 1, 1, "m")),
        RhaiError::Runtime(located("a", 1, 1, "m")),
        RhaiError::UnknownName {
            path: "p".into(),
            src: "a".into(),
        },
        RhaiError::WrongType {
            expected: "bool",
            actual: "i64".into(),
            src: "a".into(),
        },
        RhaiError::LimitExceeded {
            limit: "operations",
            src: "a".into(),
        },
    ];
    for e in all {
        assert_eq!(e.src(), "a", "{e:?}");
    }
}

#[test]
fn rhai_error_is_a_std_error() {
    fn takes_error(_: &dyn std::error::Error) {}
    takes_error(&RhaiError::Runtime(located("a", 0, 0, "m")));
}
