//! Argv-token parser: `parse_token` dispatches on the leading character
//! and structure of one argument. Grammar: see the `vista_cli` module docs.

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
    // `users[]` means "no selector", not an error.
    if inner.is_empty() {
        return Ok((prefix, None));
    }
    Ok((prefix, Some(parse_selector(inner)?)))
}

/// Parse the body of a `[…]` bracket.
pub fn parse_selector(inner: &str) -> Result<Selector> {
    let (sort, rest) = parse_selector_sort(inner)?;
    let slice_text = strip_sort_separator(inner, rest, sort.is_some())?;
    let slice = parse_selector_slice(inner, slice_text)?;
    Ok(Selector { sort, slice })
}

fn parse_selector_sort(inner: &str) -> Result<(Option<(String, Direction)>, &str)> {
    let (dir, rest) = if let Some(rest) = inner.strip_prefix('+') {
        (Direction::Asc, rest)
    } else if let Some(rest) = inner.strip_prefix('-') {
        (Direction::Desc, rest)
    } else {
        return Ok((None, inner));
    };
    let (field, rest) = take_field(rest);
    if field.is_empty() {
        let sign = if matches!(dir, Direction::Asc) {
            "+"
        } else {
            "-"
        };
        return Err(error!(
            "Bracket `[±…]` needs a field name",
            sign = sign,
            bracket = inner
        ));
    }
    Ok((Some((field.to_string(), dir)), rest))
}

fn strip_sort_separator<'a>(inner: &str, rest: &'a str, has_sort: bool) -> Result<&'a str> {
    if !has_sort {
        return Ok(rest);
    }
    match rest.strip_prefix(':') {
        Some(s) => Ok(s),
        None if rest.is_empty() => Ok(""),
        None => Err(error!(
            "Bracket expected `:` after sort field",
            bracket = inner
        )),
    }
}

fn parse_selector_slice(inner: &str, slice_text: &str) -> Result<Option<Slice>> {
    if slice_text.is_empty() {
        return Ok(None);
    }
    if let Some((start_str, end_str)) = slice_text.split_once(':') {
        let start = parse_slice_index(inner, start_str, "start", 0)?;
        let end = if end_str.is_empty() {
            None
        } else {
            Some(parse_slice_index(inner, end_str, "end", 0)?)
        };
        return Ok(Some(Slice::Range { start, end }));
    }
    let n = slice_text.parse::<usize>().map_err(|_| {
        error!(
            "Bracket index must be a non-negative integer",
            bracket = inner,
            index = slice_text
        )
    })?;
    Ok(Some(Slice::Index(n)))
}

fn parse_slice_index(inner: &str, raw: &str, edge: &str, default_empty: usize) -> Result<usize> {
    if raw.is_empty() {
        return Ok(default_empty);
    }
    raw.parse::<usize>().map_err(|_| {
        error!(
            "Bracket has a bad slice bound",
            bracket = inner,
            edge = edge,
            value = raw
        )
    })
}

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
    if let Some(rest) = arg.strip_prefix(':') {
        return parse_relation_token(arg, rest);
    }
    if arg.starts_with('[') {
        return parse_standalone_bracket(arg);
    }
    if let Some(rest) = arg.strip_prefix('=') {
        return parse_columns_token(arg, rest);
    }
    if let Some(rest) = arg.strip_prefix('?') {
        return parse_search_token(arg, rest);
    }
    if let Some(rest) = arg.strip_prefix('@') {
        return parse_aggregate_token(rest);
    }
    if let Some(eq_pos) = arg.find('=') {
        return parse_condition_token(arg, eq_pos);
    }
    if let Some(token) = parse_nullary_condition(arg)? {
        return Ok(token);
    }
    parse_name_or_locator(arg)
}

fn parse_relation_token(arg: &str, rest: &str) -> Result<Token> {
    let (rel, sel) = split_bracket_suffix(rest)?;
    if rel.is_empty() {
        return Err(error!("Empty relation name in token", token = arg));
    }
    Ok(Token::Relation(rel.to_string(), sel))
}

fn parse_standalone_bracket(arg: &str) -> Result<Token> {
    let (stem, sel) = split_bracket_suffix(arg)?;
    if !stem.is_empty() {
        return Err(error!("Malformed bracket token", token = arg));
    }
    let sel = sel.ok_or_else(|| error!("Empty bracket in token", token = arg))?;
    Ok(Token::Bracket(sel))
}

fn parse_columns_token(arg: &str, rest: &str) -> Result<Token> {
    let (cols_part, sel) = split_bracket_suffix(rest)?;
    if cols_part.is_empty() {
        return Err(error!(
            "Empty column list in token — write `=col1,col2`",
            token = arg
        ));
    }
    let cols: Vec<String> = cols_part
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if cols.is_empty() {
        return Err(error!("Empty column list in token", token = arg));
    }
    Ok(Token::Columns(cols, sel))
}

fn parse_search_token(arg: &str, rest: &str) -> Result<Token> {
    let query = strip_quotes(rest);
    if query.is_empty() {
        return Err(error!("Empty search query in token", token = arg));
    }
    Ok(Token::Search(query.to_string()))
}

fn parse_aggregate_token(rest: &str) -> Result<Token> {
    let (op_str, field_str) = match rest.split_once(':') {
        Some((o, f)) => (o, Some(f)),
        None => (rest, None),
    };
    let op = AggregateOp::parse(op_str)
        .ok_or_else(|| error!("Unknown aggregate", aggregate = op_str))?;
    if !matches!(op, AggregateOp::Count) && field_str.is_none() {
        return Err(error!(
            "Aggregate needs a field — write `@<aggregate>:<column>`",
            aggregate = op.name()
        ));
    }
    Ok(Token::Aggregate {
        op,
        field: field_str.map(str::to_string),
    })
}

/// `eq_pos` is the index of the first `=`.
fn parse_condition_token(arg: &str, eq_pos: usize) -> Result<Token> {
    let field_part = &arg[..eq_pos];
    let value_part = &arg[eq_pos + 1..];
    if field_part.is_empty() {
        return Err(error!("Empty field name in token", token = arg));
    }
    let (field, op) = parse_field_and_op(field_part)?;
    let (value_str, sel) = split_value_and_bracket(value_part)?;
    // Raw text is kept for column-typed re-coercion, except for `#literal`
    // values and `Op::In` lists.
    let is_typed_escape = value_str.starts_with('#');
    let value = match op {
        Op::In => CborValue::Array(parse_value_list(value_str)?),
        _ => parse_value(value_str)?,
    };
    let value_raw = match op {
        Op::In => None,
        _ if is_typed_escape => None,
        _ => Some(value_str.to_string()),
    };
    Ok(Token::OpCondition {
        field,
        op,
        value: Some(value),
        value_raw,
        selector: sel,
    })
}

/// `field:null` / `field:notnull` look like locators; they are told apart
/// by the suffix being a nullary op. `None` means "not a condition".
fn parse_nullary_condition(arg: &str) -> Result<Option<Token>> {
    let Some(colon) = arg.rfind(':') else {
        return Ok(None);
    };
    let (before, after) = arg.split_at(colon);
    let after = &after[1..];
    let Some(op) = Op::parse(after) else {
        return Ok(None);
    };
    if !op.is_nullary() || before.is_empty() {
        return Ok(None);
    }
    Ok(Some(Token::OpCondition {
        field: before.to_string(),
        op,
        value: None,
        value_raw: None,
        selector: None,
    }))
}

/// A model name, or a locator when the bare stem contains `:`.
fn parse_name_or_locator(arg: &str) -> Result<Token> {
    // The selector is peeled off first: a `:` inside a bracket
    // (`users[+name:0]`) must not make this a locator.
    let (stem, sel) = split_bracket_suffix(arg)?;
    if stem.is_empty() {
        return Err(error!("Empty model name in token", token = arg));
    }
    if stem.contains(':') {
        // A bracket glued to a locator is passed on verbatim; the factory decides.
        return Ok(Token::Locator(arg.to_string()));
    }
    Ok(Token::ModelName(stem.to_string(), sel))
}

/// Defaults to `Op::Eq` when there is no `:op` suffix.
fn parse_field_and_op(field_part: &str) -> Result<(String, Op)> {
    match field_part.split_once(':') {
        Some((field, op_str)) => {
            let op =
                Op::parse(op_str).ok_or_else(|| error!("Unknown operator", operator = op_str))?;
            if op.is_nullary() {
                return Err(error!(
                    "Operator is nullary — drop the `=` and value",
                    operator = op_str
                ));
            }
            if field.is_empty() {
                return Err(error!(
                    "Empty field name before operator",
                    operator = op_str
                ));
            }
            Ok((field.to_string(), op))
        }
        None => Ok((field_part.to_string(), Op::Eq)),
    }
}

/// Split `value[…]` into value text and an optional selector. Quoted
/// values (`"text [x]"`) and `#` JSON values (`#[1,2,3]`) may contain
/// brackets and are never split.
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
