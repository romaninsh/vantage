//! Cell lookup and comparison shared by conditions, search and ordering.

use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_types::Record;

/// A cell by column name, or by dotted path into nested maps. A column whose
/// name contains a dot wins over the path reading.
pub fn lookup<'a>(record: &'a Record<CborValue>, path: &str) -> Option<&'a CborValue> {
    if let Some(v) = record.get(path) {
        return Some(v);
    }
    let mut parts = path.split('.');
    let mut cur = record.get(parts.next()?)?;
    for part in parts {
        let CborValue::Map(entries) = cur else {
            return None;
        };
        cur = entries
            .iter()
            .find(|(k, _)| matches!(k, CborValue::Text(t) if t == part))
            .map(|(_, v)| v)?;
    }
    Some(cur)
}

fn number(v: &CborValue) -> Option<f64> {
    match v {
        CborValue::Integer(i) => Some(i128::from(*i) as f64),
        CborValue::Float(f) => Some(*f),
        _ => None,
    }
}

/// Order two non-null values of the same kind; numbers compare across int
/// and float. `None` for different kinds.
pub fn cmp_values(a: &CborValue, b: &CborValue) -> Option<Ordering> {
    match (a, b) {
        (CborValue::Integer(x), CborValue::Integer(y)) => Some(i128::from(*x).cmp(&i128::from(*y))),
        (CborValue::Text(x), CborValue::Text(y)) => Some(x.cmp(y)),
        (CborValue::Bool(x), CborValue::Bool(y)) => Some(x.cmp(y)),
        _ => number(a)?.partial_cmp(&number(b)?),
    }
}

/// Equality that treats `1` and `1.0` as equal; other values compare exactly.
pub fn values_eq(a: &CborValue, b: &CborValue) -> bool {
    match cmp_values(a, b) {
        Some(o) => o == Ordering::Equal,
        None => a == b,
    }
}

/// The text a cell offers to search and `Like`: text, or a number's decimal form.
pub fn text_of(v: &CborValue) -> Option<String> {
    match v {
        CborValue::Text(s) => Some(s.clone()),
        CborValue::Integer(i) => Some(i128::from(*i).to_string()),
        CborValue::Float(f) => Some(f.to_string()),
        _ => None,
    }
}

/// SQL-style `LIKE`, case-insensitive: `%` matches any run, `_` one character.
pub fn like(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '_' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '%' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '%')
}
