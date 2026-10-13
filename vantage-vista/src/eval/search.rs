//! The quicksearch rule over a row held in memory.

use ciborium::Value as CborValue;
use vantage_types::Record;

/// Whether `row` matches the search `needle`: a case-insensitive substring
/// of the text of any cell. An empty needle matches every row.
///
/// A driver searches server-side with a cast to text, so it also finds a
/// number, a bool, a record id and a field of an embedded object. This rule
/// reads the same values, so a table behaves alike whichever side
/// evaluates the search:
///
/// - text, integers, floats and bools are read as their text;
/// - a tag (a SurrealDB record id is `Tag(8, [table, key])`) is read
///   through to its content;
/// - arrays and maps are read item by item, map values only, at any depth;
/// - null and bytes hold no text.
pub(super) fn matches_search(needle: &str, row: &Record<CborValue>) -> bool {
    let needle = needle.to_lowercase();
    needle.is_empty() || row.values().any(|cell| contains(cell, &needle))
}

/// Whether `needle`, already lower case, is in the text of `value` or of
/// anything it holds.
fn contains(value: &CborValue, needle: &str) -> bool {
    match value {
        CborValue::Text(s) => s.to_lowercase().contains(needle),
        CborValue::Integer(i) => i128::from(*i).to_string().contains(needle),
        CborValue::Float(f) => f.to_string().contains(needle),
        CborValue::Bool(b) => b.to_string().contains(needle),
        CborValue::Tag(_, inner) => contains(inner, needle),
        CborValue::Array(items) => items.iter().any(|item| contains(item, needle)),
        CborValue::Map(entries) => entries.iter().any(|(_, v)| contains(v, needle)),
        _ => false,
    }
}
