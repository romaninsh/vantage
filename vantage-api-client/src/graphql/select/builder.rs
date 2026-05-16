//! Chainable builder methods for `GraphqlSelect` — convenience setters
//! that don't need to go through the `Selectable` trait. Useful inside
//! the crate (the `TableSource` impl builds queries directly) and for
//! ad-hoc test fixtures.

use vantage_expressions::Order;

use crate::graphql::condition::{FilterDialect, GraphqlCondition};
use crate::graphql::select::GraphqlSelect;

impl GraphqlSelect {
    pub fn with_root_field(mut self, name: impl Into<String>) -> Self {
        self.root_field = Some(name.into());
        self
    }

    pub fn with_operation_name(mut self, name: impl Into<String>) -> Self {
        self.operation_name = Some(name.into());
        self
    }

    pub fn with_dialect(mut self, dialect: FilterDialect) -> Self {
        self.dialect = dialect;
        self
    }

    pub fn with_filter_arg_name(mut self, name: impl Into<String>) -> Self {
        self.filter_arg_name = Some(name.into());
        self
    }

    /// Add a terminal (scalar) field to the selection set.
    pub fn with_field(mut self, name: impl Into<String>) -> Self {
        self.fields.push(name.into());
        self
    }

    /// Add a sub-selection — used for nested relationships in Phase 6.
    pub fn with_sub_selection(mut self, name: impl Into<String>, child: GraphqlSelect) -> Self {
        self.sub_selections.push((name.into(), child));
        self
    }

    pub fn with_condition(mut self, condition: GraphqlCondition) -> Self {
        self.conditions.push(condition);
        self
    }

    pub fn with_order(mut self, field: impl Into<String>, order: Order) -> Self {
        self.sort.push((field.into(), order));
        self
    }

    pub fn with_limit(mut self, limit: Option<i64>, skip: Option<i64>) -> Self {
        self.limit = limit;
        self.skip = skip;
        self
    }
}
