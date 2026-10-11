//! Client-side quicksearch and ordering over a listed collection.

use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::SortDirection;

type Rows = Vec<(String, Record<CborValue>)>;

/// Keep the rows matching `search` (case-insensitive, against the id and
/// every text field), then sort them by `order`. With no order the listing's
/// own order is kept; the sort is stable, so equal keys keep it too.
pub(super) fn refine(
    mut rows: Rows,
    search: Option<&str>,
    order: Option<&(String, SortDirection)>,
) -> Rows {
    if let Some(needle) = search {
        let needle = needle.to_lowercase();
        rows.retain(|(id, record)| row_matches(id, record, &needle));
    }
    if let Some((field, dir)) = order {
        rows.sort_by(|(_, a), (_, b)| {
            let ord = cmp_values(a.get(field), b.get(field));
            match dir {
                SortDirection::Ascending => ord,
                SortDirection::Descending => ord.reverse(),
            }
        });
    }
    rows
}

/// Row `id` of `rows`, when it is there.
pub(super) fn pick(rows: Rows, id: &str) -> Option<Record<CborValue>> {
    rows.into_iter()
        .find_map(|(row_id, record)| (row_id == id).then_some(record))
}

/// The preview note: what is applied to the listed rows in memory.
pub(super) fn describe<'a>(
    conditions: impl Iterator<Item = &'a str>,
    search: Option<&str>,
    order: Option<&(String, SortDirection)>,
) -> String {
    let mut parts = Vec::new();
    let fields: Vec<&str> = conditions.collect();
    if !fields.is_empty() {
        parts.push(format!("equality on {}", fields.join(", ")));
    }
    if let Some(text) = search {
        parts.push(format!("search {text:?}"));
    }
    if let Some((field, dir)) = order {
        let dir = match dir {
            SortDirection::Ascending => "asc",
            SortDirection::Descending => "desc",
        };
        parts.push(format!("order by {field} {dir}"));
    }
    let applied = if parts.is_empty() {
        "nothing".to_string()
    } else {
        parts.join("; ")
    };
    format!("the whole collection is listed; applied in memory afterwards: {applied}")
}

/// Quicksearch predicate: the needle (already lowercased) appears in the id
/// or in any text-valued field of the record.
fn row_matches(id: &str, record: &Record<CborValue>, needle: &str) -> bool {
    if id.to_lowercase().contains(needle) {
        return true;
    }
    record.values().any(|v| match v {
        CborValue::Text(s) => s.to_lowercase().contains(needle),
        _ => false,
    })
}

/// Order two optional CBOR values. Missing fields sort last; types compare
/// within kind, with a stable fallback across kinds.
fn cmp_values(a: Option<&CborValue>, b: Option<&CborValue>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => match (a, b) {
            (CborValue::Integer(x), CborValue::Integer(y)) => i128::from(*x).cmp(&i128::from(*y)),
            (CborValue::Float(x), CborValue::Float(y)) => {
                x.partial_cmp(y).unwrap_or(Ordering::Equal)
            }
            (CborValue::Text(x), CborValue::Text(y)) => x.cmp(y),
            (CborValue::Bool(x), CborValue::Bool(y)) => x.cmp(y),
            // Mixed or other kinds: a textual rendering keeps the sort total
            // and deterministic.
            _ => format!("{a:?}").cmp(&format!("{b:?}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Rows {
        ["web-b", "Sidecar", "web-a"]
            .into_iter()
            .map(|name| {
                let record: Record<CborValue> = [("name".to_string(), CborValue::from(name))]
                    .into_iter()
                    .collect();
                (format!("id-{name}"), record)
            })
            .collect()
    }

    fn names(rows: &Rows) -> Vec<String> {
        rows.iter()
            .map(|(_, r)| match r.get("name") {
                Some(CborValue::Text(s)) => s.clone(),
                other => panic!("name: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn search_is_case_insensitive_over_text_fields() {
        let found = refine(rows(), Some("sidecar"), None);
        assert_eq!(names(&found), ["Sidecar"]);
    }

    #[test]
    fn order_sorts_both_ways_and_none_keeps_listing_order() {
        let asc = ("name".to_string(), SortDirection::Ascending);
        let desc = ("name".to_string(), SortDirection::Descending);
        assert_eq!(
            names(&refine(rows(), None, None)),
            ["web-b", "Sidecar", "web-a"]
        );
        assert_eq!(
            names(&refine(rows(), None, Some(&asc))),
            ["Sidecar", "web-a", "web-b"]
        );
        assert_eq!(
            names(&refine(rows(), None, Some(&desc))),
            ["web-b", "web-a", "Sidecar"]
        );
    }

    /// `get` picks from the refined rows, so a row the search drops is not
    /// found by id.
    #[test]
    fn a_searched_set_misses_the_ids_it_drops() {
        let found = refine(rows(), Some("sidecar"), None);
        assert!(pick(found.clone(), "id-Sidecar").is_some());
        assert!(pick(found, "id-web-a").is_none());
    }

    #[test]
    fn missing_fields_sort_last() {
        assert_eq!(
            cmp_values(None, Some(&CborValue::from(1_i64))),
            Ordering::Greater
        );
    }
}
