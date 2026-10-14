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
pub(super) fn path_get<'a>(row: &'a Record<CborValue>, path: &str) -> Option<&'a CborValue> {
    let mut segments = path.split('.');
    let mut current = row.get(segments.next()?)?;
    for segment in segments {
        let CborValue::Map(entries) = current else {
            return None;
        };
        current = entries.iter().find_map(|(key, value)| match key {
            CborValue::Text(name) if name == segment => Some(value),
            _ => None,
        })?;
    }
    Some(current)
}
