//! Sorting-related support data-types

/// Sort direction for ordering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// Order specification combining expression and direction
#[derive(Debug, Clone)]
pub struct OrderBy<E> {
    pub expression: E,
    pub direction: SortDirection,
}

impl<E> OrderBy<E> {
    /// Create a new OrderBy with ascending direction
    pub fn ascending(expression: E) -> Self {
        Self {
            expression,
            direction: SortDirection::Ascending,
        }
    }

    /// Create a new OrderBy with descending direction
    pub fn descending(expression: E) -> Self {
        Self {
            expression,
            direction: SortDirection::Descending,
        }
    }
}

/// Handle for temporary order clauses that can be removed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderHandle(pub(super) i64);
