//! Client-side quicksearch and ordering over a listed collection.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{SortDirection, VistaMetadata, VistaQuery};

type Rows = Vec<(String, Record<CborValue>)>;

/// Keep the rows `refinement` matches (its search, by vista's
/// [`eval`](vantage_vista::eval) rule), then sort them by its order, ties
/// broken on id. With no order the listing's own order is kept.
pub(super) fn refine(
    rows: Rows,
    refinement: &VistaQuery,
    metadata: &VistaMetadata,
) -> Result<Rows> {
    let mut kept = Rows::with_capacity(rows.len());
    for (id, record) in rows {
        if refinement.matches(&record, metadata)? {
            kept.push((id, record));
        }
    }
    refinement.sort_rows(&mut kept, metadata);
    Ok(kept)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn refine(rows: Rows, search: Option<&str>, order: Option<&(String, SortDirection)>) -> Rows {
        let mut refinement = VistaQuery::new();
        refinement.search = search.map(str::to_string);
        refinement.order = order.cloned();
        super::refine(rows, &refinement, &VistaMetadata::new()).unwrap()
    }

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
    fn missing_fields_sort_last_both_ways() {
        let mut with_gap = rows();
        with_gap.insert(0, ("id-none".to_string(), Record::new()));
        for dir in [SortDirection::Ascending, SortDirection::Descending] {
            let order = ("name".to_string(), dir);
            let sorted = refine(with_gap.clone(), None, Some(&order));
            assert_eq!(sorted.last().unwrap().0, "id-none", "{dir:?}");
        }
    }
}
