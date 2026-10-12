//! [`Page::from_listing`]: a window cut from a shell's full listing, for a
//! shell that can't push the window to its backend.

use ciborium::Value as CborValue;
use vantage_types::Record;

use super::Page;

impl Page {
    /// Rows `offset..offset + limit` of `rows`, the shell's full listing
    /// (an `IndexMap` or a `Vec` of `(id, record)`) in the set's order, with
    /// the listing's length as the total.
    ///
    /// Any window is cut. `Vista` refuses an `offset` above 0 before the
    /// shell is called unless the shell advertises
    /// [`Window`](crate::Capability::Window), so advertising it stays the
    /// shell's own choice.
    pub fn from_listing<I>(rows: I, offset: usize, limit: usize) -> Page
    where
        I: IntoIterator<Item = (String, Record<CborValue>)>,
        I::IntoIter: ExactSizeIterator,
    {
        let rows = rows.into_iter();
        let total = rows.len() as i64;
        Page::new(rows.skip(offset).take(limit).collect()).with_total(total)
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::*;

    fn rows(ids: &[&str]) -> IndexMap<String, Record<CborValue>> {
        ids.iter()
            .map(|id| (id.to_string(), Record::new()))
            .collect()
    }

    fn ids(page: &Page) -> Vec<&str> {
        page.rows.iter().map(|(id, _)| id.as_str()).collect()
    }

    #[test]
    fn the_leading_window_truncates_in_order() {
        let page = Page::from_listing(rows(&["c", "a", "b"]), 0, 2);
        assert_eq!(ids(&page), ["c", "a"]);
        assert_eq!(page.total, Some(3));

        let all = Page::from_listing(rows(&["c", "a"]), 0, 5);
        assert_eq!(all.rows.len(), 2);
    }

    #[test]
    fn a_later_window_skips_the_offset() {
        let page = Page::from_listing(rows(&["a", "b", "c", "d"]), 1, 2);
        assert_eq!(ids(&page), ["b", "c"]);
        assert_eq!(page.total, Some(4));

        let past_the_end = Page::from_listing(rows(&["a"]), 3, 2);
        assert!(past_the_end.rows.is_empty());
        assert_eq!(past_the_end.total, Some(1));
    }
}
