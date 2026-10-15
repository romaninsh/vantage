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

/// The identifier `text` names, with `"…"` or `` `…` `` quoting removed and
/// doubled quotes collapsed; `None` when it is anything but one identifier.
fn unquote_identifier(text: &str) -> Option<String> {
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
