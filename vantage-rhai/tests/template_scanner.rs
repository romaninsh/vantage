use vantage_rhai::RhaiError;
use vantage_rhai::template::{Part, split, strip_single_wrapper};

fn lit(s: &str) -> Part {
    Part::Lit(s.to_string())
}

fn hole(s: &str) -> Part {
    Part::Hole(s.to_string())
}

#[test]
fn text_without_holes_is_one_literal() {
    assert_eq!(split("Hello").unwrap(), vec![lit("Hello")]);
}

#[test]
fn an_empty_template_has_no_parts() {
    assert_eq!(split("").unwrap(), vec![]);
}

#[test]
fn text_and_holes_alternate_in_order() {
    assert_eq!(
        split("Order ${args.id} in ${cur}").unwrap(),
        vec![lit("Order "), hole("args.id"), lit(" in "), hole("cur")]
    );
}

#[test]
fn a_hole_keeps_the_text_between_its_braces_untrimmed() {
    assert_eq!(split("${ a + b }").unwrap(), vec![hole(" a + b ")]);
}

#[test]
fn adjacent_holes_have_no_literal_between_them() {
    assert_eq!(split("${a}${b}").unwrap(), vec![hole("a"), hole("b")]);
}

#[test]
fn a_dollar_without_a_brace_is_plain_text() {
    assert_eq!(split("cost: $5").unwrap(), vec![lit("cost: $5")]);
}

#[test]
fn nested_braces_stay_inside_the_hole() {
    assert_eq!(
        split(r#"${ if x { "a" } else { "b" } }!"#).unwrap(),
        vec![hole(r#" if x { "a" } else { "b" } "#), lit("!")]
    );
}

#[test]
fn braces_in_string_literals_are_text() {
    assert_eq!(
        split(r#"${ "}" + "{" + "\"}" }"#).unwrap(),
        vec![hole(r#" "}" + "{" + "\"}" "#)]
    );
}

#[test]
fn braces_in_char_literals_are_text() {
    assert_eq!(split("${ '}' }").unwrap(), vec![hole(" '}' ")]);
    assert_eq!(
        split(r"${ '\'' + '{' }").unwrap(),
        vec![hole(r" '\'' + '{' ")]
    );
}

#[test]
fn braces_in_backtick_strings_are_text() {
    assert_eq!(split("${ `{` }").unwrap(), vec![hole(" `{` ")]);
    assert_eq!(split("${ `}` }").unwrap(), vec![hole(" `}` ")]);
    assert_eq!(
        split(r#"${ "a" + `b}c` }"#).unwrap(),
        vec![hole(r#" "a" + `b}c` "#)]
    );
}

#[test]
fn interpolation_inside_a_backtick_string_does_not_end_the_hole() {
    assert_eq!(split("${ `x${y}z` }").unwrap(), vec![hole(" `x${y}z` ")]);
}

#[test]
fn braces_in_block_comments_are_text() {
    assert_eq!(
        split("${ 1 /* } */ + 2 }").unwrap(),
        vec![hole(" 1 /* } */ + 2 ")]
    );
}

#[test]
fn braces_in_line_comments_are_text() {
    assert_eq!(
        split("${ 1 + // }\n 2 }").unwrap(),
        vec![hole(" 1 + // }\n 2 ")]
    );
}

#[test]
fn text_after_a_hole_with_a_closing_brace_is_still_literal() {
    assert_eq!(split("${a} } {").unwrap(), vec![hole("a"), lit(" } {")]);
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
