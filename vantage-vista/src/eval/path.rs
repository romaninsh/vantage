//! Reading one cell of a row: a flat key, or a dotted path into nested maps.

use ciborium::Value as CborValue;
use vantage_types::Record;

use crate::metadata::VistaMetadata;

/// The cell `column` names in `row`.
///
/// A column without a dot, or one `metadata` declares with a dot in its
/// name (SQL's imported `author.name`), is a flat key. Any other dotted
/// column is a path into nested maps, read segment by segment, so a literal
/// `"a.b"` key in the row never stands in for the path.
pub fn cell_get<'a>(
    row: &'a Record<CborValue>,
    column: &str,
    metadata: &VistaMetadata,
) -> Option<&'a CborValue> {
    if !column.contains('.') || metadata.columns.contains_key(column) {
        row.get(column)
    } else {
        path_get(row, column)
    }
}

/// The value at dotted `path` inside nested maps: the first segment is a
/// key of `row`, each later one a text key of the map before it. A path
/// without a dot is a plain key.
pub(crate) fn path_get<'a>(row: &'a Record<CborValue>, path: &str) -> Option<&'a CborValue> {
    let mut segments = path.split('.');
    let mut current = row.get(segments.next()?)?;
    for segment in segments {
        let CborValue::Map(entries) = current else {
            return None;
        };
        current = entry(entries, segment)?;
    }
    Some(current)
}

/// For a `column` [`cell_get`] reads as a path: the prefix whose cell holds
/// a value that isn't a map (nor null), so the rest of the path can't be
/// read in memory. `None` when the path reads, or stops at an absent key or
/// a null.
pub(crate) fn blocked_at<'a>(
    row: &Record<CborValue>,
    column: &'a str,
    metadata: &VistaMetadata,
) -> Option<&'a str> {
    if !column.contains('.') || metadata.columns.contains_key(column) {
        return None;
    }
    path_blocked_at(row, column)
}

fn path_blocked_at<'a>(row: &Record<CborValue>, path: &'a str) -> Option<&'a str> {
    let mut segments = path.split('.');
    let first = segments.next()?;
    let mut current = row.get(first)?;
    let mut end = first.len();
    for segment in segments {
        let CborValue::Map(entries) = current else {
            return (!current.is_null()).then(|| &path[..end]);
        };
        current = entry(entries, segment)?;
        end += 1 + segment.len();
    }
    None
}

fn entry<'a>(entries: &'a [(CborValue, CborValue)], segment: &str) -> Option<&'a CborValue> {
    entries.iter().find_map(|(key, value)| match key {
        CborValue::Text(name) if name == segment => Some(value),
        _ => None,
    })
}
