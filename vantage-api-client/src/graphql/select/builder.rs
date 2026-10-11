//! Chainable builder methods for `GraphqlSelect` — convenience setters
//! that don't need to go through the `Selectable` trait. Used by the
//! `TableSource` impl, which builds queries directly, and by ad-hoc
//! queries posted through `GraphqlApi::post_graphql`.

use vantage_expressions::Order;

use crate::graphql::condition::{FilterDialect, GraphqlCondition};
use crate::graphql::select::GraphqlSelect;

impl GraphqlSelect {
    /// Set the root field on the `Query` type (`launches`).
    pub fn with_root_field(mut self, name: impl Into<String>) -> Self {
        self.root_field = Some(name.into());
        self
    }

    /// Name the operation: `query <name>(…) { … }`.
    pub fn with_operation_name(mut self, name: impl Into<String>) -> Self {
        self.operation_name = Some(name.into());
        self
    }

    /// Pick the dialect conditions render in.
    pub fn with_dialect(mut self, dialect: FilterDialect) -> Self {
        self.dialect = dialect;
        self
    }

    /// Override the filter argument name (the dialect's `where` / `find`).
    pub fn with_filter_arg_name(mut self, name: impl Into<String>) -> Self {
        self.filter_arg_name = Some(name.into());
        self
    }

    /// Add a terminal (scalar) field to the selection set. A dotted path
    /// (`rocket.name`) renders as a nested selection.
    pub fn with_field(mut self, name: impl Into<String>) -> Self {
        self.fields.push(name.into());
        self
    }

    /// Literal arguments always passed to the root field. Expects a JSON
    /// object; anything else is ignored at render time.
    pub fn with_root_args(mut self, args: serde_json::Value) -> Self {
        self.root_args = Some(args);
        self
    }

    /// Envelope the rows sit inside — `["edges", "node"]` wraps the
    /// selection set in `edges { node { … } }`.
    pub fn with_response_path(mut self, path: Vec<String>) -> Self {
        self.response_path = path;
        self
    }

    /// Add a nested selection set under `name`, rendered with its own
    /// inline filter and paging arguments.
    pub fn with_sub_selection(mut self, name: impl Into<String>, child: GraphqlSelect) -> Self {
        self.sub_selections.push((name.into(), child));
        self
    }

    /// Add a condition; every condition is ANDed into the filter argument.
    pub fn with_condition(mut self, condition: GraphqlCondition) -> Self {
        self.conditions.push(condition);
        self
    }

    /// Add a sort key (rendered as `order_by:` on Hasura only).
    pub fn with_order(mut self, field: impl Into<String>, order: Order) -> Self {
        self.sort.push((field.into(), order));
        self
    }

    /// Set the page size and offset, sent as `$limit` / `$offset`.
    pub fn with_limit(mut self, limit: Option<i64>, skip: Option<i64>) -> Self {
        self.limit = limit;
        self.skip = skip;
        self
    }

    /// Read column `name` from server field `field`: the selection set asks
    /// for `name: field` and conditions and sort keys on `name` are sent as
    /// `field`. Both must be undotted GraphQL names.
    pub(crate) fn with_alias(mut self, name: impl Into<String>, field: impl Into<String>) -> Self {
        self.aliases.insert(name.into(), field.into());
        self
    }
}
