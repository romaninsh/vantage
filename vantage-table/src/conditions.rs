use vantage_expressions::{Expression, ExpressiveEnum};

/// Handle for temporary conditions that can be removed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionHandle(pub(crate) i64);

impl ConditionHandle {
    pub(crate) fn new(id: i64) -> Self {
        Self(id)
    }
}

/// When `expr` is a literal `column = value` — the shape `column.eq(value)`
/// builds on every expression backend — return the column and the value.
///
/// The left side must be one bare or quoted identifier (`parent`,
/// `"parent"`, `` `parent` ``). A table-qualified one (`"t"."parent"`) is a
/// correlation, not a set definition, and reads as `None`, as does any
/// operator other than `=`.
pub fn literal_equality<V: Clone>(expr: &Expression<V>) -> Option<(String, V)> {
    if expr.template.trim() != "{} = {}" || expr.parameters.len() != 2 {
        return None;
    }
    let column = match &expr.parameters[0] {
        ExpressiveEnum::Nested(lhs) if lhs.parameters.is_empty() => {
            unquote_identifier(&lhs.template)?
        }
        _ => return None,
    };
    let value = match &expr.parameters[1] {
        ExpressiveEnum::Scalar(v) => v.clone(),
        ExpressiveEnum::Nested(rhs) if rhs.template.trim() == "{}" && rhs.parameters.len() == 1 => {
            match &rhs.parameters[0] {
                ExpressiveEnum::Scalar(v) => v.clone(),
                _ => return None,
            }
        }
        _ => return None,
    };
    Some((column, value))
}

/// Operators [`condition_column_of`] reads as narrowing a column to values.
/// Negations (`!=`, `<>`, `NOT IN`, `NOT LIKE`) are left out: they narrow
/// the set *away* from a value, which no "requires a condition on this
/// column" check should accept.
const COLUMN_OPERATORS: [&str; 7] = ["=", ">", ">=", "<", "<=", "IN", "LIKE"];

/// The column `expr` narrows to values when it is `column <op> value` —
/// `=`, comparisons, `LIKE`, `IN` (with or without parentheses around the
/// operand) — with one bare or quoted identifier on the left and a value on
/// the right: a scalar, a `{}`-wrapped scalar, a deferred value, and for
/// `IN` also a value list or a subquery.
///
/// Anything else reads as `None`: a raw expression, `AND`/`OR`, a negation,
/// a table-qualified column, a column or arbitrary expression on the right
/// (`col = other_col`, `col = 7 OR 1 = 1`).
pub fn condition_column_of<V>(expr: &Expression<V>) -> Option<String> {
    if expr.parameters.len() != 2 {
        return None;
    }
    let rest = expr.template.trim().strip_prefix("{}")?;
    let operand_at = rest.find("{}")?;
    let operator = rest[..operand_at].trim();
    let (operator, parenthesised) = match operator.strip_suffix('(') {
        Some(op) => (op.trim(), true),
        None => (operator, false),
    };
    let tail = rest[operand_at + 2..].trim();
    if tail != if parenthesised { ")" } else { "" } {
        return None;
    }
    let operator = operator.to_ascii_uppercase();
    if !COLUMN_OPERATORS.contains(&operator.as_str()) {
        return None;
    }
    let operand = &expr.parameters[1];
    let narrows = if operator == "IN" {
        is_value(operand) || is_value_set(operand)
    } else {
        is_value(operand)
    };
    if !narrows {
        return None;
    }
    match &expr.parameters[0] {
        ExpressiveEnum::Nested(lhs) if lhs.parameters.is_empty() => {
            unquote_identifier(&lhs.template)
        }
        _ => None,
    }
}

/// A scalar, a deferred value, or either wrapped in any number of `{}`
/// templates — the operand shape `col.eq(v)` builds.
fn is_value<V>(operand: &ExpressiveEnum<V>) -> bool {
    match operand {
        ExpressiveEnum::Scalar(_) | ExpressiveEnum::Deferred(_) => true,
        ExpressiveEnum::Nested(e) => {
            e.template.trim() == "{}" && e.parameters.len() == 1 && is_value(&e.parameters[0])
        }
    }
}

/// An `IN` operand that names a set of values: a list of values
/// (`{}, {}, …`, or empty / `NULL` for no values) or a subquery.
fn is_value_set<V>(operand: &ExpressiveEnum<V>) -> bool {
    let ExpressiveEnum::Nested(e) = operand else {
        return false;
    };
    let template = e.template.trim();
    if template == "{}" && e.parameters.len() == 1 {
        return is_value(&e.parameters[0]) || is_value_set(&e.parameters[0]);
    }
    if template.is_empty() || template.eq_ignore_ascii_case("NULL") {
        return e.parameters.is_empty();
    }
    if is_subquery(e) {
        return true;
    }
    let slots: Vec<&str> = template.split(',').map(str::trim).collect();
    slots.iter().all(|s| *s == "{}")
        && slots.len() == e.parameters.len()
        && e.parameters.iter().all(is_value)
}

/// A `SELECT` (or `WITH … SELECT`) query. SQL selects render as
/// `{}SELECT …` with the CTE slot first, so a leading `{}` is accepted
/// only when its parameter is empty or a `WITH` clause.
fn is_subquery<V>(e: &Expression<V>) -> bool {
    let mut text = e.template.trim_start().trim_start_matches('(').trim_start();
    if let Some(rest) = text.strip_prefix("{}") {
        match e.parameters.first() {
            Some(ExpressiveEnum::Nested(cte))
                if cte.template.trim().is_empty() || starts_with_keyword(&cte.template, "WITH") => {
            }
            _ => return false,
        }
        text = rest;
    }
    starts_with_keyword(text, "SELECT") || starts_with_keyword(text, "WITH")
}

fn starts_with_keyword(text: &str, keyword: &str) -> bool {
    let text = text.trim_start();
    text.get(..keyword.len())
        .is_some_and(|w| w.eq_ignore_ascii_case(keyword))
        && !text[keyword.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The identifier `text` names, with `"…"` or `` `…` `` quoting removed and
/// doubled quotes collapsed; `None` when it is anything but one identifier.
pub fn unquote_identifier(text: &str) -> Option<String> {
    let text = text.trim();
    for q in ['"', '`'] {
        if let Some(inner) = text.strip_prefix(q).and_then(|t| t.strip_suffix(q)) {
            let doubled: String = [q, q].iter().collect();
            if inner.is_empty() || inner.replace(&doubled, "").contains(q) {
                return None;
            }
            return Some(inner.replace(&doubled, &q.to_string()));
        }
    }
    let mut chars = text.chars();
    let first = chars.next()?;
    ((first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_'))
    .then(|| text.to_string())
}
