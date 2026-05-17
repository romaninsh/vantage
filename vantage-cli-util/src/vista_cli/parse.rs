//! Argv-token parser. One pure function per public form, plus the
//! top-level `parse_token` that dispatches based on the leading
//! character and structural cues.

use vantage_core::{Result, error};

use super::token::{AggregateOp, Direction, Op, Selector, Slice, Token};
use super::value::{parse_value, parse_value_list};

/// Split a trailing `[…]` selector from an outer token. Returns
/// `(prefix, Some(selector))` if the token ends in a parseable
/// bracket, else `(input, None)`.
pub fn split_bracket_suffix(s: &str) -> Result<(&str, Option<Selector>)> {
    let Some(stripped) = s.strip_suffix(']') else {
        return Ok((s, None));
    };
    let Some(open) = stripped.rfind('[') else {
        return Ok((s, None));
    };
    let prefix = &stripped[..open];
    let inner = &stripped[open + 1..];
    // An empty bracket on a model name (e.g. `users[]`) is treated as
    // "no selector", not an error — matches the legacy lenience for
    // trailing decoration that doesn't pin anything down.
    if inner.is_empty() {
        return Ok((prefix, None));
    }
    Ok((prefix, Some(parse_selector(inner)?)))
}

/// Parse the body of a `[…]` bracket.
pub fn parse_selector(inner: &str) -> Result<Selector> {
    // 1. Optional sort prefix.
    let (sort, rest) = if let Some(rest) = inner.strip_prefix('+') {
        let (field, rest) = take_field(rest);
        if field.is_empty() {
            return Err(error!(format!(
                "Bracket `[+…]` needs a field name, got `[{inner}]`"
            )));
        }
        (Some((field.to_string(), Direction::Asc)), rest)
    } else if let Some(rest) = inner.strip_prefix('-') {
        let (field, rest) = take_field(rest);
        if field.is_empty() {
            return Err(error!(format!(
                "Bracket `[-…]` needs a field name, got `[{inner}]`"
            )));
        }
        (Some((field.to_string(), Direction::Desc)), rest)
    } else {
        (None, inner)
    };

    // 2. Optional separator between sort and slice. If sort is present
    //    and there's more text, a leading `:` separates them. If sort
    //    is absent, the rest is the slice text directly.
    let slice_text = if sort.is_some() {
        match rest.strip_prefix(':') {
            Some(s) => s,
            None if rest.is_empty() => "",
            None => {
                return Err(error!(format!(
                    "Bracket `[{inner}]`: expected `:` after sort field"
                )));
            }
        }
    } else {
        rest
    };

    let slice = if slice_text.is_empty() {
        None
    } else if let Some((start_str, end_str)) = slice_text.split_once(':') {
        let start = if start_str.is_empty() {
            0
        } else {
            start_str.parse::<usize>().map_err(|_| {
                error!(format!(
                    "Bracket `[{inner}]`: bad slice start `{start_str}`"
                ))
            })?
        };
        let end =
            if end_str.is_empty() {
                None
            } else {
                Some(end_str.parse::<usize>().map_err(|_| {
                    error!(format!("Bracket `[{inner}]`: bad slice end `{end_str}`"))
                })?)
            };
        Some(Slice::Range { start, end })
    } else {
        let n = slice_text.parse::<usize>().map_err(|_| {
            error!(format!(
                "Bracket `[{inner}]`: index `{slice_text}` must be a non-negative integer"
            ))
        })?;
        Some(Slice::Index(n))
    };

    Ok(Selector { sort, slice })
}

/// Take alphanumeric/underscore field-name characters from the front of
/// `s`; return `(field, rest)`. Stops at `:`.
fn take_field(s: &str) -> (&str, &str) {
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .unwrap_or(s.len());
    s.split_at(end)
}

/// Top-level: turn one argv token into a structured `Token`.
pub fn parse_token(arg: &str) -> Result<Token> {
    if arg.is_empty() {
        return Err(error!("Empty argument"));
    }

    // `:relation` — relation traversal, possibly glued with `[…]`.
    if let Some(rest) = arg.strip_prefix(':') {
        let (rel, sel) = split_bracket_suffix(rest)?;
        if rel.is_empty() {
            return Err(error!(format!("Empty relation name in token `{arg}`")));
        }
        return Ok(Token::Relation(rel.to_string(), sel));
    }

    // Standalone `[…]` bracket on the current vista.
    if arg.starts_with('[') {
        // Reuse split_bracket_suffix by prefixing an empty stem.
        let (stem, sel) = split_bracket_suffix(arg)?;
        if !stem.is_empty() {
            // Shouldn't reach — `[` at position 0 means stem is "".
            return Err(error!(format!("Malformed bracket token `{arg}`")));
        }
        let sel = sel.ok_or_else(|| error!(format!("Empty bracket in token `{arg}`")))?;
        return Ok(Token::Bracket(sel));
    }

    // `=col1,col2,…[N]` — column override.
    if let Some(rest) = arg.strip_prefix('=') {
        let (cols_part, sel) = split_bracket_suffix(rest)?;
        if cols_part.is_empty() {
            return Err(error!(format!(
                "Empty column list in token `{arg}` — write `=col1,col2`"
            )));
        }
        let cols: Vec<String> = cols_part
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if cols.is_empty() {
            return Err(error!(format!("Empty column list in token `{arg}`")));
        }
        return Ok(Token::Columns(cols, sel));
    }

    // `?keyword` or `?"two words"` — search.
    if let Some(rest) = arg.strip_prefix('?') {
        let query = strip_quotes(rest);
        if query.is_empty() {
            return Err(error!(format!("Empty search query in token `{arg}`")));
        }
        return Ok(Token::Search(query.to_string()));
    }

    // `@op[:field]` — aggregate.
    if let Some(rest) = arg.strip_prefix('@') {
        let (op_str, field_str) = match rest.split_once(':') {
            Some((o, f)) => (o, Some(f)),
            None => (rest, None),
        };
        let op = AggregateOp::parse(op_str)
            .ok_or_else(|| error!(format!("Unknown aggregate `@{op_str}`")))?;
        // `@count` may omit the field; everything else requires one.
        if !matches!(op, AggregateOp::Count) && field_str.is_none() {
            return Err(error!(format!(
                "`@{}` needs a field — write `@{}:<column>`",
                op.name(),
                op.name()
            )));
        }
        return Ok(Token::Aggregate {
            op,
            field: field_str.map(str::to_string),
        });
    }

    // Look for `=` outside of a JSON-typed value to decide condition vs.
    // model-name/locator. The `=` must be at the field-name boundary,
    // not inside the value, so we use the *first* `=` (Rust's `find`).
    if let Some(eq_pos) = arg.find('=') {
        let field_part = &arg[..eq_pos];
        let value_part = &arg[eq_pos + 1..];
        if field_part.is_empty() {
            return Err(error!(format!("Empty field name in token `{arg}`")));
        }
        let (field, op) = parse_field_and_op(field_part)?;
        let (value_str, sel) = split_value_and_bracket(value_part)?;
        let value = match op {
            Op::In => CborValue::Array(parse_value_list(value_str)?),
            _ => parse_value(value_str)?,
        };
        return Ok(Token::OpCondition {
            field,
            op,
            value: Some(value),
            selector: sel,
        });
    }

    // Nullary op (`field:null`, `field:notnull`) — `:` is present but
    // no `=`. Disambiguate from a locator (which also has `:`) by
    // checking whether the suffix is a known nullary op.
    if let Some(colon) = arg.rfind(':') {
        let (before, after) = arg.split_at(colon);
        let after = &after[1..]; // skip the `:`
        if let Some(op) = Op::parse(after)
            && op.is_nullary()
            && !before.is_empty()
        {
            return Ok(Token::OpCondition {
                field: before.to_string(),
                op,
                value: None,
                selector: None,
            });
        }
    }

    // Peel off any trailing `[…]` selector so we look at the bare stem
    // for the model-vs-locator decision. Without this step a bracket
    // containing `:` (e.g. `users[+name:0]`) would shift the whole
    // token into the locator branch below.
    let (stem, sel) = split_bracket_suffix(arg)?;
    if stem.is_empty() {
        return Err(error!(format!("Empty model name in token `{arg}`")));
    }
    if stem.contains(':') {
        // Locator with a bracket-glued suffix is semantically odd (a
        // locator is already a single record), but rather than carve
        // out a special error here we hand the verbatim token to the
        // factory and let it decide.
        return Ok(Token::Locator(arg.to_string()));
    }
    Ok(Token::ModelName(stem.to_string(), sel))
}

/// Split a `field[:op]` left-of-equals into field + op. Defaults to
/// `Op::Eq` when no `:op` suffix is present.
fn parse_field_and_op(field_part: &str) -> Result<(String, Op)> {
    match field_part.split_once(':') {
        Some((field, op_str)) => {
            let op = Op::parse(op_str)
                .ok_or_else(|| error!(format!("Unknown operator `:{op_str}=`")))?;
            if op.is_nullary() {
                return Err(error!(format!(
                    "Operator `:{op_str}` is nullary — drop the `=` and value"
                )));
            }
            if field.is_empty() {
                return Err(error!(format!(
                    "Empty field name before operator `:{op_str}=`"
                )));
            }
            Ok((field.to_string(), op))
        }
        None => Ok((field_part.to_string(), Op::Eq)),
    }
}

/// Split the value side of `field=value[…]` into the raw value text
/// and an optional trailing bracket selector. Two escapes suppress
/// bracket splitting because the value itself may legitimately contain
/// `[` / `]`:
///   - Fully quoted values (`field="text [with brackets]"`).
///   - JSON-typed values (`field=#[1,2,3]`, `field=#{"k":"v"}`).
fn split_value_and_bracket(value_part: &str) -> Result<(&str, Option<Selector>)> {
    if value_part.starts_with('"') && value_part.ends_with('"') && value_part.len() >= 2 {
        return Ok((&value_part[1..value_part.len() - 1], None));
    }
    if value_part.starts_with('#') {
        return Ok((value_part, None));
    }
    split_bracket_suffix(value_part)
}

fn strip_quotes(s: &str) -> &str {
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

use ciborium::Value as CborValue;

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Token {
        parse_token(s).unwrap_or_else(|e| panic!("parse `{s}` failed: {e:?}"))
    }

    // ── Existing token shapes (regression) ─────────────────────────────────

    #[test]
    fn model_name_simple() {
        match parse("users") {
            Token::ModelName(n, sel) => {
                assert_eq!(n, "users");
                assert!(sel.is_none());
            }
            t => panic!("expected ModelName, got {t:?}"),
        }
    }

    #[test]
    fn model_name_with_index() {
        match parse("users[0]") {
            Token::ModelName(n, sel) => {
                assert_eq!(n, "users");
                assert_eq!(
                    sel,
                    Some(Selector {
                        sort: None,
                        slice: Some(Slice::Index(0)),
                    })
                );
            }
            t => panic!("expected ModelName with selector, got {t:?}"),
        }
    }

    #[test]
    fn relation_with_index() {
        match parse(":albums[2]") {
            Token::Relation(r, sel) => {
                assert_eq!(r, "albums");
                assert_eq!(sel.unwrap().slice, Some(Slice::Index(2)));
            }
            t => panic!("expected Relation, got {t:?}"),
        }
    }

    #[test]
    fn columns_token() {
        match parse("=name,age") {
            Token::Columns(cols, sel) => {
                assert_eq!(cols, vec!["name", "age"]);
                assert!(sel.is_none());
            }
            t => panic!("expected Columns, got {t:?}"),
        }
    }

    // ── Operators ──────────────────────────────────────────────────────────

    #[test]
    fn eq_with_autodetect() {
        match parse("name=alice") {
            Token::OpCondition {
                field, op, value, ..
            } => {
                assert_eq!(field, "name");
                assert_eq!(op, Op::Eq);
                assert_eq!(value, Some(CborValue::Text("alice".into())));
            }
            t => panic!("expected OpCondition, got {t:?}"),
        }
    }

    #[test]
    fn quoted_value() {
        match parse("name=\"john doe\"") {
            Token::OpCondition { value, .. } => {
                assert_eq!(value, Some(CborValue::Text("john doe".into())));
            }
            t => panic!("expected OpCondition, got {t:?}"),
        }
    }

    #[test]
    fn op_lt() {
        match parse("salary:lt=1000") {
            Token::OpCondition {
                field, op, value, ..
            } => {
                assert_eq!(field, "salary");
                assert_eq!(op, Op::Lt);
                assert!(matches!(value, Some(CborValue::Integer(_))));
            }
            t => panic!("expected OpCondition, got {t:?}"),
        }
    }

    #[test]
    fn op_in_comma_list() {
        match parse("dept:in=eng,ops,qa") {
            Token::OpCondition { op, value, .. } => {
                assert_eq!(op, Op::In);
                let arr = match value {
                    Some(CborValue::Array(items)) => items,
                    other => panic!("expected Array value, got {other:?}"),
                };
                assert_eq!(arr.len(), 3);
                assert_eq!(arr[1], CborValue::Text("ops".into()));
            }
            t => panic!("expected OpCondition, got {t:?}"),
        }
    }

    #[test]
    fn op_in_json_array() {
        match parse("dept:in=#[1, 2, \"three\"]") {
            Token::OpCondition { value, .. } => {
                let arr = match value {
                    Some(CborValue::Array(items)) => items,
                    other => panic!("expected Array value, got {other:?}"),
                };
                assert_eq!(arr.len(), 3);
                assert_eq!(arr[2], CborValue::Text("three".into()));
            }
            t => panic!("expected OpCondition, got {t:?}"),
        }
    }

    #[test]
    fn op_null_nullary() {
        match parse("manager_id:null") {
            Token::OpCondition {
                field, op, value, ..
            } => {
                assert_eq!(field, "manager_id");
                assert_eq!(op, Op::IsNull);
                assert!(value.is_none());
            }
            t => panic!("expected nullary OpCondition, got {t:?}"),
        }
    }

    #[test]
    fn op_notnull_nullary() {
        match parse("email:notnull") {
            Token::OpCondition { op, .. } => assert_eq!(op, Op::IsNotNull),
            t => panic!("expected nullary OpCondition, got {t:?}"),
        }
    }

    #[test]
    fn op_unknown_errors() {
        assert!(parse_token("salary:foo=1").is_err());
    }

    // ── Typed JSON values ──────────────────────────────────────────────────

    #[test]
    fn typed_bool() {
        match parse("is_active=#true") {
            Token::OpCondition { value, .. } => assert_eq!(value, Some(CborValue::Bool(true))),
            t => panic!("expected OpCondition, got {t:?}"),
        }
    }

    #[test]
    fn typed_string_overrides_int_lookalike() {
        match parse("note=#\"42\"") {
            Token::OpCondition { value, .. } => {
                assert_eq!(value, Some(CborValue::Text("42".into())));
            }
            t => panic!("expected OpCondition, got {t:?}"),
        }
    }

    #[test]
    fn typed_null() {
        match parse("data=#null") {
            Token::OpCondition { value, .. } => assert_eq!(value, Some(CborValue::Null)),
            t => panic!("expected OpCondition, got {t:?}"),
        }
    }

    // ── Bracket selector — sort + slice ────────────────────────────────────

    #[test]
    fn bracket_single_index() {
        let s = parse_selector("5").unwrap();
        assert_eq!(s.sort, None);
        assert_eq!(s.slice, Some(Slice::Index(5)));
    }

    #[test]
    fn bracket_slice_range() {
        let s = parse_selector("5:15").unwrap();
        assert_eq!(s.sort, None);
        assert_eq!(
            s.slice,
            Some(Slice::Range {
                start: 5,
                end: Some(15),
            })
        );
    }

    #[test]
    fn bracket_slice_open_start() {
        let s = parse_selector(":20").unwrap();
        assert_eq!(
            s.slice,
            Some(Slice::Range {
                start: 0,
                end: Some(20),
            })
        );
    }

    #[test]
    fn bracket_slice_open_end() {
        let s = parse_selector("100:").unwrap();
        assert_eq!(
            s.slice,
            Some(Slice::Range {
                start: 100,
                end: None,
            })
        );
    }

    #[test]
    fn bracket_sort_asc() {
        let s = parse_selector("+name").unwrap();
        assert_eq!(s.sort, Some(("name".to_string(), Direction::Asc)));
        assert_eq!(s.slice, None);
    }

    #[test]
    fn bracket_sort_desc() {
        let s = parse_selector("-salary").unwrap();
        assert_eq!(s.sort, Some(("salary".to_string(), Direction::Desc)));
        assert_eq!(s.slice, None);
    }

    #[test]
    fn bracket_sort_plus_index() {
        // The user's example: highest-salary row → narrow → traverse.
        let s = parse_selector("+salary:0").unwrap();
        assert_eq!(s.sort, Some(("salary".to_string(), Direction::Asc)));
        assert_eq!(s.slice, Some(Slice::Index(0)));
    }

    #[test]
    fn bracket_sort_plus_range() {
        let s = parse_selector("+name:5:15").unwrap();
        assert_eq!(s.sort, Some(("name".to_string(), Direction::Asc)));
        assert_eq!(
            s.slice,
            Some(Slice::Range {
                start: 5,
                end: Some(15),
            })
        );
    }

    #[test]
    fn bracket_sort_open_end() {
        let s = parse_selector("+name:5:").unwrap();
        assert_eq!(
            s.slice,
            Some(Slice::Range {
                start: 5,
                end: None,
            })
        );
    }

    #[test]
    fn bracket_standalone_token() {
        match parse("[+salary:0]") {
            Token::Bracket(sel) => {
                assert_eq!(sel.sort, Some(("salary".to_string(), Direction::Asc)));
                assert_eq!(sel.slice, Some(Slice::Index(0)));
            }
            t => panic!("expected Bracket, got {t:?}"),
        }
    }

    #[test]
    fn bracket_bad_field() {
        assert!(parse_selector("+:5").is_err());
        assert!(parse_selector("+").is_err());
        assert!(parse_selector("abc").is_err());
    }

    // ── Search ─────────────────────────────────────────────────────────────

    #[test]
    fn search_simple() {
        match parse("?keyword") {
            Token::Search(s) => assert_eq!(s, "keyword"),
            t => panic!("expected Search, got {t:?}"),
        }
    }

    #[test]
    fn search_quoted() {
        match parse("?\"two words\"") {
            Token::Search(s) => assert_eq!(s, "two words"),
            t => panic!("expected Search, got {t:?}"),
        }
    }

    #[test]
    fn search_empty_errors() {
        assert!(parse_token("?").is_err());
    }

    // ── Aggregates ─────────────────────────────────────────────────────────

    #[test]
    fn aggregate_sum() {
        match parse("@sum:price") {
            Token::Aggregate { op, field } => {
                assert_eq!(op, AggregateOp::Sum);
                assert_eq!(field.as_deref(), Some("price"));
            }
            t => panic!("expected Aggregate, got {t:?}"),
        }
    }

    #[test]
    fn aggregate_count_no_field() {
        match parse("@count") {
            Token::Aggregate { op, field } => {
                assert_eq!(op, AggregateOp::Count);
                assert!(field.is_none());
            }
            t => panic!("expected Aggregate, got {t:?}"),
        }
    }

    #[test]
    fn aggregate_sum_without_field_errors() {
        assert!(parse_token("@sum").is_err());
    }

    #[test]
    fn aggregate_unknown_errors() {
        assert!(parse_token("@avg:x").is_err());
    }

    // ── Locator ────────────────────────────────────────────────────────────

    #[test]
    fn locator_arn() {
        match parse("arn:aws:iam::123:user/alice") {
            Token::Locator(s) => assert_eq!(s, "arn:aws:iam::123:user/alice"),
            t => panic!("expected Locator, got {t:?}"),
        }
    }

    #[test]
    fn locator_surreal_thing() {
        match parse("user:abc123") {
            Token::Locator(s) => assert_eq!(s, "user:abc123"),
            t => panic!("expected Locator, got {t:?}"),
        }
    }

    #[test]
    fn locator_urn() {
        match parse("urn:isbn:0451450523") {
            Token::Locator(s) => assert_eq!(s, "urn:isbn:0451450523"),
            t => panic!("expected Locator, got {t:?}"),
        }
    }

    #[test]
    fn locator_vs_relation_disambig() {
        // `:rel` is a relation, `name:rel` is a locator.
        match parse(":rel") {
            Token::Relation(r, _) => assert_eq!(r, "rel"),
            t => panic!("expected Relation, got {t:?}"),
        }
        match parse("name:rel") {
            Token::Locator(s) => assert_eq!(s, "name:rel"),
            t => panic!("expected Locator, got {t:?}"),
        }
    }

    #[test]
    fn condition_value_with_colon_stays_condition() {
        // `field=user:abc` — the `:` is inside the value, the `=`
        // separates field from value. Should not be misread as locator.
        match parse("ref=user:abc") {
            Token::OpCondition { field, value, .. } => {
                assert_eq!(field, "ref");
                assert_eq!(value, Some(CborValue::Text("user:abc".into())));
            }
            t => panic!("expected OpCondition, got {t:?}"),
        }
    }
}
