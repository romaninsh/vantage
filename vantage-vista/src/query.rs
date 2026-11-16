//! [`VistaQuery`]: the clauses applied to a Vista, as a plain value.

use ciborium::Value as CborValue;

use crate::{filter::FilterOp, sort::SortDirection};

mod key;

/// `column <op> value`. Equality is [`FilterOp::Eq`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Condition {
    pub column: String,
    pub op: FilterOp,
    pub value: CborValue,
}

impl Condition {
    pub fn new(column: impl Into<String>, op: FilterOp, value: impl Into<CborValue>) -> Self {
        Self {
            column: column.into(),
            op,
            value: value.into(),
        }
    }
}

/// Every clause applied through a [`Vista`](crate::Vista): its conditions,
/// search and order (`order` first, ties broken by each `then_by` in turn),
/// plus the keys of its driver-native conditions
/// ([`NativeCondition::key`](crate::NativeCondition::key)). A native
/// condition itself is opaque, so only its key is recorded, and a
/// `VistaQuery` that holds native keys can't be re-applied to another Vista.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct VistaQuery {
    pub conditions: Vec<Condition>,
    pub search: Option<String>,
    pub order: Option<(String, SortDirection)>,
    /// Columns that break ties left by `order`, in turn. Empty when `order`
    /// is `None`.
    pub then_by: Vec<(String, SortDirection)>,
    pub native: Vec<String>,
}

impl VistaQuery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_condition(mut self, condition: Condition) -> Self {
        self.conditions.push(condition);
        self
    }

    pub fn with_search(mut self, text: impl Into<String>) -> Self {
        self.search = Some(text.into());
        self
    }

    /// Order by `column`, replacing the whole order (tie-breakers included).
    pub fn with_order(mut self, column: impl Into<String>, dir: SortDirection) -> Self {
        self.order = Some((column.into(), dir));
        self.then_by.clear();
        self
    }

    /// Break the ties of the current order by `column`; with no order yet,
    /// order by it.
    pub fn then_order(mut self, column: impl Into<String>, dir: SortDirection) -> Self {
        match self.order {
            None => self.order = Some((column.into(), dir)),
            Some(_) => self.then_by.push((column.into(), dir)),
        }
        self
    }

    /// The whole order: the primary column, then each tie-breaker.
    pub fn orders(&self) -> impl Iterator<Item = (&str, SortDirection)> {
        self.order
            .iter()
            .chain(&self.then_by)
            .map(|(column, dir)| (column.as_str(), *dir))
    }

    /// The whole order of `other` (tie-breakers included) in place of this
    /// one's.
    pub fn copy_order_from(&mut self, other: &VistaQuery) {
        self.order = other.order.clone();
        self.then_by = other.then_by.clone();
    }

    /// No clause of any kind, order included.
    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
            && self.search.is_none()
            && self.order.is_none()
            && self.native.is_empty()
    }

    /// Whether a clause limits which rows are in the set. Order doesn't.
    pub(crate) fn narrows(&self) -> bool {
        !self.conditions.is_empty() || self.search.is_some() || !self.native.is_empty()
    }

    /// A stable string identifying this query, for cache identity. The same
    /// clauses give the same key whatever order the conditions (or native
    /// conditions) were added in, and different clauses give different keys:
    /// every part is length-prefixed and every value type-tagged, so no
    /// column name, text or value can be mistaken for a separator. Combine it
    /// with the Vista's name to key a cache across tables.
    pub fn key(&self) -> String {
        key::render(self)
    }
}
