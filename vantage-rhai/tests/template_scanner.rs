use vantage_rhai::RhaiError;
use vantage_rhai::template::{Part, split, strip_single_wrapper};

fn lit(s: &str) -> Part {
    Part::Lit(s.to_string())
}

fn hole(s: &str) -> Part {
    Part::Hole(s.to_string())
}

#[test]
fn an_empty_template_has_no_parts() {
    assert_eq!(split("").unwrap(), vec![]);
}

#[test]
fn text_and_holes_alternate_in_order() {
    assert_eq!(
        split("Order ${ args.id }${cur} costs $5 } {").unwrap(),
        vec![
            lit("Order "),
            hole(" args.id "),
            hole("cur"),
            lit(" costs $5 } {"),
        ]
    );
}

#[test]
fn braces_inside_literals_comments_and_nested_blocks_stay_in_the_hole() {
    let template = concat!(
        r#"${ "}" + "{" + "\"}" }"#,
        r"${ '}' + '\'' + '{' }",
        "${ `{` + `}` + `x${y}z` }",
        "${ 1 /* } */ + 2 }",
        "${ 1 + // }\n 2 }",
        r#"${ if x { "a" } else { "b" } }!"#,
    );
    assert_eq!(
        split(template).unwrap(),
        vec![
            hole(r#" "}" + "{" + "\"}" "#),
            hole(r" '}' + '\'' + '{' "),
            hole(" `{` + `}` + `x${y}z` "),
            hole(" 1 /* } */ + 2 "),
            hole(" 1 + // }\n 2 "),
            hole(r#" if x { "a" } else { "b" } "#),
            lit("!"),
        ]
    );
}

#[test]
fn an_unterminated_hole_is_a_syntax_error_with_its_position() {
    let err = split("a\nbb ${x").unwrap_err();
    let RhaiError::Syntax(l) = &err else {
        panic!("expected Syntax, got {err:?}")
    };
    assert_eq!((l.line, l.column), (2, 4));
    assert!(l.message.contains("unterminated"), "{}", l.message);
    assert_eq!(err.src(), "a\nbb ${x");
}

#[test]
fn an_unterminated_hole_column_counts_characters_not_bytes() {
    let err = split("café ${x").unwrap_err();
    let RhaiError::Syntax(l) = err else {
        panic!("expected Syntax")
    };
    assert_eq!((l.line, l.column), (1, 6));
}

#[test]
fn a_brace_swallowed_by_a_string_leaves_the_hole_unterminated() {
    assert!(split(r#"${ "}" "#).is_err());
}

#[test]
fn an_unterminated_hole_after_a_complete_one_is_still_an_error() {
    let err = split("${a} and ${b").unwrap_err();
    let RhaiError::Syntax(l) = err else {
        panic!("expected Syntax")
    };
    assert_eq!((l.line, l.column), (1, 10));
}

#[test]
fn a_wrapper_around_the_whole_string_is_removed() {
    assert_eq!(strip_single_wrapper("${ a + b }").trim(), "a + b");
}

#[test]
fn a_wrapper_may_contain_braces() {
    assert_eq!(
        strip_single_wrapper("${ if x { 1 } else { 2 } }").trim(),
        "if x { 1 } else { 2 }"
    );
}

#[test]
fn a_string_without_a_wrapper_keeps_its_content() {
    assert_eq!(strip_single_wrapper("  a + b  ").trim(), "a + b");
}

#[test]
fn two_holes_are_not_a_wrapper() {
    assert_eq!(strip_single_wrapper("${a}${b}"), "${a}${b}");
}

#[test]
fn text_around_a_hole_is_not_a_wrapper() {
    assert_eq!(strip_single_wrapper("x ${a}"), "x ${a}");
    assert_eq!(strip_single_wrapper("${a} x"), "${a} x");
}
