use vantage_expressions::traits::selectable::Selectable;

/// Pagination configuration for tables
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    page: i64,
    items_per_page: i64,
    /// Explicit skip for a random-access `[offset, offset+limit)` window. When
    /// set, `skip()` returns it verbatim instead of deriving it from `page` —
    /// so the offset need not be a multiple of the page size.
    offset: Option<i64>,
}

impl Pagination {
    /// Create new pagination with page number and items per page
    pub fn new(page: i64, items_per_page: i64) -> Self {
        Self {
            page: page.max(1),
            items_per_page: items_per_page.max(1),
            offset: None,
        }
    }

    /// Create pagination for an explicit `[offset, offset + limit)` window
    /// (random access). Unlike [`new`](Self::new), the offset is independent of
    /// the page size, so any absolute row range maps directly onto a query.
    pub fn window(offset: i64, limit: i64) -> Self {
        Self {
            page: 1,
            items_per_page: limit.max(1),
            offset: Some(offset.max(0)),
        }
    }

    /// Set the current page number (1-based)
    pub fn set_page(&mut self, page: i64) {
        self.page = page.max(1);
    }

    /// Set items per page
    /// When changing page size, adjusts current page to keep focused item visible
    pub fn set_ipp(&mut self, items_per_page: i64) {
        let items_per_page = items_per_page.max(1);

        let first_item_index = (self.page - 1) * self.items_per_page;
        self.page = (first_item_index / items_per_page) + 1;
        self.items_per_page = items_per_page;
    }

    /// Get the current page number (1-based)
    pub fn get_page(&self) -> i64 {
        self.page
    }

    /// Get items per page
    pub fn get_ipp(&self) -> i64 {
        self.items_per_page
    }

    /// Calculate limit value for queries
    pub fn limit(&self) -> i64 {
        self.items_per_page
    }

    /// Calculate skip/offset value for queries
    pub fn skip(&self) -> i64 {
        self.offset.unwrap_or((self.page - 1) * self.items_per_page)
    }

    /// Apply pagination to a select query
    pub fn apply_on_select<S, E>(&self, select: &mut S)
    where
        S: Selectable<E>,
    {
        select.set_limit(Some(self.limit()), Some(self.skip()));
    }
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            page: 1,
            items_per_page: 50,
            offset: None,
        }
    }
}
