//! The canonical rendering behind [`VistaQuery::key`].
//!
//! Every string goes out as `<len>:<text>`, every value as a type tag plus
//! length-prefixed parts, and every list as its length plus its elements.
//! That makes the rendering uniquely decodable, so distinct queries can't
//! render alike.

use ciborium::Value as CborValue;

use super::VistaQuery;
use crate::sort::SortDirection;

pub(super) fn render(query: &VistaQuery) -> String {
    let mut conditions: Vec<String> = query
        .conditions
        .iter()
        .map(|c| {
            let mut s = part(&c.column);
            s.push_str(&part(c.op.key_symbol()));
            s.push_str(&part(&value(&c.value)));
            s
        })
        .collect();
    conditions.sort();
    let mut native = query.native.clone();
    native.sort();

    let mut out = String::from("c");
    out.push_str(&list(&conditions));
    out.push('q');
    out.push_str(&option(query.search.as_deref()));
    out.push('s');
    let order = query.order.as_ref().map(|(column, dir)| {
        let dir = match dir {
            SortDirection::Ascending => "asc",
            SortDirection::Descending => "desc",
        };
        format!("{}{}", part(column), part(dir))
    });
    out.push_str(&option(order.as_deref()));
    out.push('n');
    out.push_str(&list(&native));
    out
}

fn part(s: &str) -> String {
    format!("{}:{s}", s.len())
}

fn list(items: &[String]) -> String {
    let mut out = part(&items.len().to_string());
    for item in items {
        out.push_str(&part(item));
    }
    out
}

fn option(s: Option<&str>) -> String {
    match s {
        None => "-".to_string(),
        Some(s) => format!("+{}", part(s)),
    }
}

/// A type tag and the value's length-prefixed payload. Map entries are
/// sorted, so their order never affects the result.
fn value(v: &CborValue) -> String {
    match v {
        CborValue::Null => "z".to_string(),
        CborValue::Bool(b) => format!("b{}", part(&b.to_string())),
        CborValue::Integer(n) => format!("i{}", part(&i128::from(*n).to_string())),
        CborValue::Float(f) => format!("f{}", part(&f.to_string())),
        CborValue::Text(s) => format!("t{}", part(s)),
        CborValue::Bytes(bytes) => {
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            format!("x{}", part(&hex))
        }
        CborValue::Tag(tag, inner) => format!("g{}{}", part(&tag.to_string()), part(&value(inner))),
        CborValue::Array(items) => {
            let items: Vec<String> = items.iter().map(value).collect();
            format!("a{}", list(&items))
        }
        CborValue::Map(pairs) => {
            let mut entries: Vec<String> = pairs
                .iter()
                .map(|(k, v)| format!("{}{}", part(&value(k)), part(&value(v))))
                .collect();
            entries.sort();
            format!("m{}", list(&entries))
        }
        other => format!("?{}", part(&format!("{other:?}"))),
    }
}
