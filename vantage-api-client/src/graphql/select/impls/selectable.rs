//! `Selectable<AnyGraphqlType, GraphqlCondition>` for `GraphqlSelect`.
//!
//! Most methods are direct field accessors. Grouping and `DISTINCT` are
//! no-ops (GraphQL has no spelling for either). The
//! as_field/as_count/as_sum/as_max/as_min constructors return placeholder
//! expressions in Hasura's `<table>_aggregate { aggregate { … } }` shape,
//! since aggregate semantics depend on the schema.

use vantage_expressions::traits::selectable::SourceRef;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum, Order, Selectable};

use crate::graphql::condition::GraphqlCondition;
use crate::graphql::select::GraphqlSelect;
use crate::graphql::types::AnyGraphqlType;

impl Selectable<AnyGraphqlType, GraphqlCondition> for GraphqlSelect {
    fn add_source(&mut self, source: impl Into<SourceRef<AnyGraphqlType>>, _alias: Option<String>) {
        let source_ref = source.into().into_expressive_enum();
        if let ExpressiveEnum::Scalar(val) = source_ref
            && let Some(name) = val.try_get::<String>()
        {
            self.root_field = Some(name);
        }
    }

    fn add_field(&mut self, field: impl Into<String>) {
        self.fields.push(field.into());
    }

    fn add_expression(&mut self, _expression: impl Expressive<AnyGraphqlType>) {
        // A selection set holds field names only, so an expression is dropped.
    }

    fn add_where_condition(&mut self, condition: impl Into<GraphqlCondition>) {
        self.conditions.push(condition.into());
    }

    /// No GraphQL spelling: ignored.
    fn set_distinct(&mut self, _distinct: bool) {}

    fn add_order_by(&mut self, order: impl Into<GraphqlCondition>, direction: Order) {
        // A Field condition carries the sort column as its `field`; any other
        // condition has no column to sort by and is dropped.
        let cond = order.into();
        if let GraphqlCondition::Field(fc) = cond {
            self.sort.push((fc.field, direction));
        }
    }

    /// No GraphQL spelling: ignored.
    fn add_group_by(&mut self, _expression: impl Expressive<AnyGraphqlType>) {}

    fn set_limit(&mut self, limit: Option<i64>, skip: Option<i64>) {
        self.limit = limit;
        self.skip = skip;
    }

    fn clear_fields(&mut self) {
        self.fields.clear();
    }

    fn clear_where_conditions(&mut self) {
        self.conditions.clear();
    }

    fn clear_order_by(&mut self) {
        self.sort.clear();
    }

    fn clear_group_by(&mut self) {}

    fn has_fields(&self) -> bool {
        !self.fields.is_empty()
    }

    fn has_where_conditions(&self) -> bool {
        !self.conditions.is_empty()
    }

    fn has_order_by(&self) -> bool {
        !self.sort.is_empty()
    }

    fn has_group_by(&self) -> bool {
        false
    }

    fn is_distinct(&self) -> bool {
        false
    }

    fn get_limit(&self) -> Option<i64> {
        self.limit
    }

    fn get_skip(&self) -> Option<i64> {
        self.skip
    }

    fn as_field(&self, _field: impl Into<String>) -> Expression<AnyGraphqlType> {
        // Without the schema's aggregate field names this can only preview
        // the query.
        Expression::new(self.preview(), vec![])
    }

    fn as_count(&self) -> Expression<AnyGraphqlType> {
        Expression::new(
            format!(
                "{}_aggregate {{ aggregate {{ count }} }}",
                self.root_field.clone().unwrap_or_default()
            ),
            vec![],
        )
    }

    fn as_sum(&self, column: impl Expressive<AnyGraphqlType>) -> Expression<AnyGraphqlType> {
        Expression::new(
            format!(
                "{}_aggregate {{ aggregate {{ sum {{ {} }} }} }}",
                self.root_field.clone().unwrap_or_default(),
                column.preview()
            ),
            vec![],
        )
    }

    fn as_max(&self, column: impl Expressive<AnyGraphqlType>) -> Expression<AnyGraphqlType> {
        Expression::new(
            format!(
                "{}_aggregate {{ aggregate {{ max {{ {} }} }} }}",
                self.root_field.clone().unwrap_or_default(),
                column.preview()
            ),
            vec![],
        )
    }

    fn as_min(&self, column: impl Expressive<AnyGraphqlType>) -> Expression<AnyGraphqlType> {
        Expression::new(
            format!(
                "{}_aggregate {{ aggregate {{ min {{ {} }} }} }}",
                self.root_field.clone().unwrap_or_default(),
                column.preview()
            ),
            vec![],
        )
    }
}

impl Expressive<AnyGraphqlType> for GraphqlSelect {
    fn expr(&self) -> Expression<AnyGraphqlType> {
        Expression::new(self.preview(), vec![])
    }
}
