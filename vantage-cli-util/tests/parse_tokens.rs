//! Argv-token grammar of the Vista CLI: every token form documented on
//! `vista_cli` parses into the matching `Token`, and malformed forms error.

use ciborium::Value as CborValue;
use vantage_cli_util::vista_cli::{
    AggregateOp, Direction, Op, Selector, Slice, Token, parse_selector, parse_token,
    split_bracket_suffix,
};

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn index_selector(n: usize) -> Selector {
    Selector {
        sort: None,
        slice: Some(Slice::Index(n)),
    }
}

fn range_selector(start: usize, end: Option<usize>) -> Selector {
    Selector {
        sort: None,
        slice: Some(Slice::Range { start, end }),
    }
}

fn sort_selector(field: &str, dir: Direction, slice: Option<Slice>) -> Selector {
    Selector {
        sort: Some((field.to_string(), dir)),
        slice,
    }
}

fn condition(field: &str, op: Op, value: CborValue, raw: Option<&str>) -> Token {
    Token::OpCondition {
        field: field.to_string(),
        op,
        value: Some(value),
        value_raw: raw.map(str::to_string),
        selector: None,
    }
}

// ─── Model names, locators ─────────────────────────────────────────────────

#[test]
fn bare_name_is_model_name() {
    assert_eq!(
        parse_token("users").unwrap(),
        Token::ModelName("users".into(), None)
    );
}

#[test]
fn glued_index_attaches_selector_to_model_name() {
    assert_eq!(
        parse_token("users[0]").unwrap(),
        Token::ModelName("users".into(), Some(index_selector(0)))
    );
}

#[test]
fn glued_sort_and_index_attach_to_model_name() {
    assert_eq!(
        parse_token("users[+name:0]").unwrap(),
        Token::ModelName(
            "users".into(),
            Some(sort_selector("name", Direction::Asc, Some(Slice::Index(0))))
        )
    );
}

#[test]
fn tokens_with_a_scheme_prefix_are_locators_kept_verbatim() {
    for locator in ["arn:aws:s3:::bucket", "user:abc123", "urn:isbn:0451450523"] {
        assert_eq!(
            parse_token(locator).unwrap(),
            Token::Locator(locator.to_string())
        );
    }
}

#[test]
fn empty_argument_is_an_error() {
    assert!(parse_token("").is_err());
}

// ─── Bracket selectors ─────────────────────────────────────────────────────

#[test]
fn standalone_bracket_index() {
    assert_eq!(
        parse_token("[5]").unwrap(),
        Token::Bracket(index_selector(5))
    );
}

#[test]
fn standalone_bracket_ranges() {
    assert_eq!(
        parse_token("[5:15]").unwrap(),
        Token::Bracket(range_selector(5, Some(15)))
    );
    assert_eq!(
        parse_token("[:5]").unwrap(),
        Token::Bracket(range_selector(0, Some(5)))
    );
    assert_eq!(
        parse_token("[5:]").unwrap(),
        Token::Bracket(range_selector(5, None))
    );
    assert_eq!(
        parse_token("[:]").unwrap(),
        Token::Bracket(range_selector(0, None))
    );
}

#[test]
fn standalone_bracket_sort_ascending_and_descending() {
    assert_eq!(
        parse_token("[+name]").unwrap(),
        Token::Bracket(sort_selector("name", Direction::Asc, None))
    );
    assert_eq!(
        parse_token("[-name]").unwrap(),
        Token::Bracket(sort_selector("name", Direction::Desc, None))
    );
}

#[test]
fn standalone_bracket_sort_then_slice() {
    assert_eq!(
        parse_token("[-salary:0]").unwrap(),
        Token::Bracket(sort_selector(
            "salary",
            Direction::Desc,
            Some(Slice::Index(0))
        ))
    );
    assert_eq!(
        parse_token("[+name:2:5]").unwrap(),
        Token::Bracket(sort_selector(
            "name",
            Direction::Asc,
            Some(Slice::Range {
                start: 2,
                end: Some(5)
            })
        ))
    );
}

#[test]
fn malformed_brackets_are_errors() {
    for bad in [
        "[]", "[+]", "[-]", "[abc]", "[+name!]", "[1:x]", "[x:1]", "[1:2:3]",
    ] {
        assert!(parse_token(bad).is_err(), "{bad} should not parse");
    }
}

#[test]
fn parse_selector_reads_the_bracket_body() {
    assert_eq!(
        parse_selector("+name:3").unwrap(),
        sort_selector("name", Direction::Asc, Some(Slice::Index(3)))
    );
    assert_eq!(parse_selector("7").unwrap(), index_selector(7));
    assert!(parse_selector("").unwrap().is_empty());
    assert!(!parse_selector("7").unwrap().is_empty());
}

#[test]
fn split_bracket_suffix_separates_prefix_and_selector() {
    assert_eq!(
        split_bracket_suffix("users[0]").unwrap(),
        ("users", Some(index_selector(0)))
    );
    assert_eq!(split_bracket_suffix("users").unwrap(), ("users", None));
    assert_eq!(split_bracket_suffix("users]").unwrap(), ("users]", None));
    assert!(split_bracket_suffix("users[abc]").is_err());
}

// ─── Relations, columns, search, aggregates ────────────────────────────────

#[test]
fn colon_prefix_is_a_relation() {
    assert_eq!(
        parse_token(":orders").unwrap(),
        Token::Relation("orders".into(), None)
    );
    assert_eq!(
        parse_token(":orders[0]").unwrap(),
        Token::Relation("orders".into(), Some(index_selector(0)))
    );
}

#[test]
fn relation_needs_a_name() {
    assert!(parse_token(":").is_err());
    assert!(parse_token(":[0]").is_err());
}

#[test]
fn equals_prefix_is_a_column_override() {
    assert_eq!(
        parse_token("=name,salary").unwrap(),
        Token::Columns(vec!["name".into(), "salary".into()], None)
    );
    assert_eq!(
        parse_token("=name[0]").unwrap(),
        Token::Columns(vec!["name".into()], Some(index_selector(0)))
    );
}

#[test]
fn column_override_needs_columns() {
    assert!(parse_token("=").is_err());
    assert!(parse_token("=,").is_err());
}

#[test]
fn question_mark_prefix_is_a_search() {
    assert_eq!(
        parse_token("?alice").unwrap(),
        Token::Search("alice".into())
    );
    assert_eq!(
        parse_token("?\"two words\"").unwrap(),
        Token::Search("two words".into())
    );
}

#[test]
fn empty_search_is_an_error() {
    assert!(parse_token("?").is_err());
    assert!(parse_token("?\"\"").is_err());
}

#[test]
fn aggregates_with_a_field() {
    for (token, op) in [
        ("@sum:salary", AggregateOp::Sum),
        ("@max:salary", AggregateOp::Max),
        ("@min:salary", AggregateOp::Min),
        ("@count:salary", AggregateOp::Count),
    ] {
        assert_eq!(
            parse_token(token).unwrap(),
            Token::Aggregate {
                op,
                field: Some("salary".into())
            }
        );
    }
}

#[test]
fn count_needs_no_field() {
    assert_eq!(
        parse_token("@count").unwrap(),
        Token::Aggregate {
            op: AggregateOp::Count,
            field: None
        }
    );
}

#[test]
fn aggregate_errors() {
    assert!(parse_token("@sum").is_err(), "sum needs a field");
    assert!(parse_token("@max").is_err(), "max needs a field");
    assert!(parse_token("@median:salary").is_err(), "unknown aggregate");
}

// ─── Conditions ────────────────────────────────────────────────────────────

#[test]
fn eq_condition_autodetects_value_and_keeps_raw_text() {
    assert_eq!(
        parse_token("age=42").unwrap(),
        condition("age", Op::Eq, int(42), Some("42"))
    );
    assert_eq!(
        parse_token("ratio=3.5").unwrap(),
        condition("ratio", Op::Eq, CborValue::Float(3.5), Some("3.5"))
    );
    assert_eq!(
        parse_token("flag=true").unwrap(),
        condition("flag", Op::Eq, CborValue::Bool(true), Some("true"))
    );
    assert_eq!(
        parse_token("name=Alice").unwrap(),
        condition("name", Op::Eq, text("Alice"), Some("Alice"))
    );
}

#[test]
fn quoted_value_keeps_spaces_and_brackets() {
    assert_eq!(
        parse_token("name=\"Alice Smith\"").unwrap(),
        condition("name", Op::Eq, text("Alice Smith"), Some("Alice Smith"))
    );
    assert_eq!(
        parse_token("note=\"a [b]\"").unwrap(),
        condition("note", Op::Eq, text("a [b]"), Some("a [b]"))
    );
}

#[test]
fn glued_selector_attaches_to_condition() {
    assert_eq!(
        parse_token("name=Alice[0]").unwrap(),
        Token::OpCondition {
            field: "name".into(),
            op: Op::Eq,
            value: Some(text("Alice")),
            value_raw: Some("Alice".into()),
            selector: Some(index_selector(0)),
        }
    );
}

#[test]
fn comparison_operators() {
    for (suffix, op) in [
        ("eq", Op::Eq),
        ("ne", Op::Ne),
        ("lt", Op::Lt),
        ("lte", Op::Lte),
        ("gt", Op::Gt),
        ("gte", Op::Gte),
        ("like", Op::Like),
    ] {
        assert_eq!(
            parse_token(&format!("age:{suffix}=30")).unwrap(),
            condition("age", op, int(30), Some("30")),
            "operator {suffix}"
        );
    }
}

#[test]
fn in_operator_takes_a_comma_list() {
    assert_eq!(
        parse_token("age:in=1,2,3").unwrap(),
        condition(
            "age",
            Op::In,
            CborValue::Array(vec![int(1), int(2), int(3)]),
            None
        )
    );
}

#[test]
fn in_operator_takes_a_json_array() {
    assert_eq!(
        parse_token("tag:in=#[\"a\",2]").unwrap(),
        condition(
            "tag",
            Op::In,
            CborValue::Array(vec![text("a"), int(2)]),
            None
        )
    );
}

#[test]
fn in_operator_rejects_non_array_json() {
    assert!(parse_token("age:in=#5").is_err());
    assert!(parse_token("age:in=#[").is_err());
}

#[test]
fn null_and_notnull_are_nullary() {
    assert_eq!(
        parse_token("deleted:null").unwrap(),
        Token::OpCondition {
            field: "deleted".into(),
            op: Op::IsNull,
            value: None,
            value_raw: None,
            selector: None,
        }
    );
    assert_eq!(
        parse_token("email:notnull").unwrap(),
        Token::OpCondition {
            field: "email".into(),
            op: Op::IsNotNull,
            value: None,
            value_raw: None,
            selector: None,
        }
    );
}

#[test]
fn nullary_operator_with_a_value_is_an_error() {
    assert!(parse_token("deleted:null=1").is_err());
}

#[test]
fn unknown_operator_is_an_error() {
    assert!(parse_token("age:bogus=1").is_err());
}

#[test]
fn hash_prefix_forces_a_json_typed_value() {
    assert_eq!(
        parse_token("flag=#true").unwrap(),
        condition("flag", Op::Eq, CborValue::Bool(true), None)
    );
    assert_eq!(
        parse_token("code=#\"42\"").unwrap(),
        condition("code", Op::Eq, text("42"), None)
    );
    assert_eq!(
        parse_token("x=#null").unwrap(),
        condition("x", Op::Eq, CborValue::Null, None)
    );
    assert_eq!(
        parse_token("x=#[1,2,3]").unwrap(),
        condition(
            "x",
            Op::Eq,
            CborValue::Array(vec![int(1), int(2), int(3)]),
            None
        )
    );
    assert_eq!(
        parse_token("x=#{\"k\":\"v\"}").unwrap(),
        condition(
            "x",
            Op::Eq,
            CborValue::Map(vec![(text("k"), text("v"))]),
            None
        )
    );
}

#[test]
fn invalid_json_literal_is_an_error() {
    assert!(parse_token("x=#{").is_err());
}

// ─── Operator and aggregate vocabulary ─────────────────────────────────────

#[test]
fn op_names_round_trip_through_parse() {
    for op in [
        Op::Eq,
        Op::Ne,
        Op::Lt,
        Op::Lte,
        Op::Gt,
        Op::Gte,
        Op::Like,
        Op::In,
        Op::IsNull,
        Op::IsNotNull,
    ] {
        assert_eq!(Op::parse(op.name()), Some(op));
    }
    assert_eq!(Op::parse("bogus"), None);
}

#[test]
fn only_null_and_notnull_are_nullary() {
    assert!(Op::IsNull.is_nullary());
    assert!(Op::IsNotNull.is_nullary());
    assert!(!Op::Eq.is_nullary());
    assert!(!Op::In.is_nullary());
}

#[test]
fn aggregate_names_round_trip_through_parse() {
    for op in [
        AggregateOp::Sum,
        AggregateOp::Max,
        AggregateOp::Min,
        AggregateOp::Count,
    ] {
        assert_eq!(AggregateOp::parse(op.name()), Some(op));
    }
    assert_eq!(AggregateOp::parse("median"), None);
}
