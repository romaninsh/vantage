//! Value comparison and column access shared by the derived shell and the
//! built-in aggregations.

use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_types::Record;

/// Resolve a column against a record, walking dotted paths (`obj.field`) into
/// nested CBOR maps. Tries the literal key first, so a column whose name
/// genuinely contains a dot still resolves.
pub fn column<'a>(record: &'a Record<CborValue>, path: &str) -> Option<&'a CborValue> {
    if let Some(value) = record.get(path) {
        return Some(value);
    }
    let mut segments = path.split('.');
    let mut current = record.get(segments.next()?)?;
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

/// Total order over CBOR scalars.
///
/// Numbers compare numerically across the integer/float boundary — never by
/// their debug rendering, which would rank `657.96` above `1826.19` because
/// `'6' > '1'`. `NaN` compares equal to everything rather than panicking a
/// sort. Values of different kinds fall back to a stable kind ranking so the
/// order is total even on ragged data.
pub fn compare(a: &CborValue, b: &CborValue) -> Ordering {
    match (a, b) {
        (CborValue::Text(l), CborValue::Text(r)) => l.cmp(r),
        (CborValue::Bool(l), CborValue::Bool(r)) => l.cmp(r),
        (CborValue::Bytes(l), CborValue::Bytes(r)) => l.cmp(r),
        _ => match (as_f64(a), as_f64(b)) {
            (Some(l), Some(r)) => l.partial_cmp(&r).unwrap_or(Ordering::Equal),
            _ => kind_rank(a).cmp(&kind_rank(b)),
        },
    }
}

/// Compare an optional cell, keeping absent values **last regardless of
/// direction**: `descending` reverses the comparison of present values only, so
/// flipping the sort never lifts blanks to the top.
pub fn compare_cells(a: Option<&CborValue>, b: Option<&CborValue>, descending: bool) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(l), Some(r)) => {
            let ordering = compare(l, r);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        }
    }
}

/// Order two rows by a column, breaking ties on id.
///
/// The tiebreak is what makes the result reproducible: without it, rows sharing
/// a sort value keep whatever order the input happened to have, so the same
/// data assembled differently renders differently — and an aggregation that
/// publishes only on change would then publish on no change at all.
pub fn compare_rows(
    (a_id, a_row): (&str, &Record<CborValue>),
    (b_id, b_row): (&str, &Record<CborValue>),
    sort_column: &str,
    descending: bool,
) -> Ordering {
    compare_cells(
        column(a_row, sort_column),
        column(b_row, sort_column),
        descending,
    )
    .then_with(|| a_id.cmp(b_id))
}

/// Numeric view of a CBOR scalar, for comparison and for the numeric
/// aggregations. `None` for anything that is not a number.
pub fn as_f64(value: &CborValue) -> Option<f64> {
    match value {
        CborValue::Integer(i) => Some(i128::from(*i) as f64),
        CborValue::Float(f) => Some(*f),
        _ => None,
    }
}

/// Case-insensitive substring match over a record's text cells, including text
/// nested one level inside maps and arrays.
pub fn matches_search(record: &Record<CborValue>, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    record.values().any(|value| contains_text(value, &needle))
}

fn contains_text(value: &CborValue, needle: &str) -> bool {
    match value {
        CborValue::Text(text) => text.to_lowercase().contains(needle),
        CborValue::Array(items) => items.iter().any(|item| contains_text(item, needle)),
        CborValue::Map(entries) => entries.iter().any(|(_, v)| contains_text(v, needle)),
        _ => false,
    }
}

/// Stable ranking across CBOR kinds, so a column holding mixed types still has
/// a total order instead of an arbitrary one.
fn kind_rank(value: &CborValue) -> u8 {
    match value {
        CborValue::Null => 0,
        CborValue::Bool(_) => 1,
        CborValue::Integer(_) | CborValue::Float(_) => 2,
        CborValue::Text(_) => 3,
        CborValue::Bytes(_) => 4,
        CborValue::Array(_) => 5,
        CborValue::Map(_) => 6,
        _ => 7,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(n: i64) -> CborValue {
        CborValue::Integer(n.into())
    }

    fn float(f: f64) -> CborValue {
        CborValue::Float(f)
    }

    fn text(s: &str) -> CborValue {
        CborValue::Text(s.to_string())
    }

    #[test]
    fn numbers_compare_numerically_not_lexicographically() {
        // The 0.6.13 regression in miniature: as debug strings, "657.96"
        // sorts above "1826.19".
        assert_eq!(compare(&float(657.96), &float(1826.19)), Ordering::Less);
        // …and across the integer/float boundary.
        assert_eq!(compare(&int(2), &float(10.5)), Ordering::Less);
        assert_eq!(compare(&float(10.5), &int(2)), Ordering::Greater);
    }

    #[test]
    fn nan_does_not_panic_and_compares_equal() {
        assert_eq!(compare(&float(f64::NAN), &int(1)), Ordering::Equal);
    }

    #[test]
    fn absent_values_sort_last_in_both_directions() {
        let one = int(1);
        for descending in [false, true] {
            assert_eq!(
                compare_cells(None, Some(&one), descending),
                Ordering::Greater
            );
            assert_eq!(compare_cells(Some(&one), None, descending), Ordering::Less);
        }
        assert_eq!(
            compare_cells(Some(&int(1)), Some(&int(2)), false),
            Ordering::Less
        );
        assert_eq!(
            compare_cells(Some(&int(1)), Some(&int(2)), true),
            Ordering::Greater
        );
    }

    #[test]
    fn ties_break_on_id_so_ordering_is_reproducible() {
        let mut a = Record::new();
        a.insert("v".to_string(), int(1));
        let mut b = Record::new();
        b.insert("v".to_string(), int(1));
        assert_eq!(
            compare_rows(("b", &a), ("a", &b), "v", false),
            Ordering::Greater
        );
        // The tiebreak is not reversed by the sort direction — only the
        // column comparison is — so the order stays deterministic either way.
        assert_eq!(
            compare_rows(("b", &a), ("a", &b), "v", true),
            Ordering::Greater
        );
    }

    #[test]
    fn dotted_paths_descend_into_nested_maps() {
        let mut record = Record::new();
        record.insert(
            "customer".to_string(),
            CborValue::Map(vec![(text("country"), text("LV"))]),
        );
        assert_eq!(column(&record, "customer.country"), Some(&text("LV")));
        assert_eq!(column(&record, "customer.missing"), None);
    }

    #[test]
    fn a_literal_dotted_key_wins_over_path_descent() {
        let mut record = Record::new();
        record.insert("a.b".to_string(), int(7));
        assert_eq!(column(&record, "a.b"), Some(&int(7)));
    }

    #[test]
    fn search_finds_nested_text() {
        let mut record = Record::new();
        record.insert("name".to_string(), text("Widget"));
        record.insert(
            "meta".to_string(),
            CborValue::Map(vec![(text("note"), text("Fragile"))]),
        );
        assert!(matches_search(&record, "widg"));
        assert!(matches_search(&record, "FRAGILE"));
        assert!(!matches_search(&record, "absent"));
    }
}
