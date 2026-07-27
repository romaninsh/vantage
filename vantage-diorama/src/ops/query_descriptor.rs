use ciborium::Value as CborValue;
use vantage_vista::SortDirection;

/// Description of the query a Scenery's list pass is about to run against the
/// master. Carries the scenery's conditions, sort, and the pagination window
/// (`offset`/`limit`) so a server-side list script can filter and order the
/// page it returns.
///
/// A two-pass `on_list_page` callback reads these fields to fetch the right
/// slice; the resulting ids feed the per-query index (keyed by
/// [`Vista::index_key`](vantage_vista::Vista::index_key) over the same
/// conditions + sort).
#[derive(Debug, Clone, Default)]
pub struct QueryDescriptor {
    pub conditions: Vec<(String, CborValue)>,
    pub sort: Option<(String, SortDirection)>,
    pub offset: usize,
    pub limit: usize,
}

impl QueryDescriptor {
    /// An empty descriptor — no conditions, no sort, window `0..0`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the pagination window for this page request.
    pub fn with_window(mut self, offset: usize, limit: usize) -> Self {
        self.offset = offset;
        self.limit = limit;
        self
    }

    /// Attach the scenery's conditions.
    pub fn with_conditions(mut self, conditions: Vec<(String, CborValue)>) -> Self {
        self.conditions = conditions;
        self
    }

    /// Attach the scenery's sort.
    pub fn with_sort(mut self, sort: Option<(String, SortDirection)>) -> Self {
        self.sort = sort;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> CborValue {
        CborValue::Text(s.into())
    }

    #[test]
    fn carries_conditions_sort_and_window() {
        let q = QueryDescriptor::new()
            .with_conditions(vec![("branch".into(), t("main"))])
            .with_sort(Some(("status".into(), SortDirection::Descending)))
            .with_window(50, 25);

        assert_eq!(q.conditions, vec![("branch".to_string(), t("main"))]);
        assert_eq!(
            q.sort,
            Some(("status".to_string(), SortDirection::Descending))
        );
        assert_eq!(q.offset, 50);
        assert_eq!(q.limit, 25);
    }

    #[test]
    fn default_is_empty_zero_window() {
        let q = QueryDescriptor::new();
        assert!(q.conditions.is_empty());
        assert!(q.sort.is_none());
        assert_eq!((q.offset, q.limit), (0, 0));
    }
}
