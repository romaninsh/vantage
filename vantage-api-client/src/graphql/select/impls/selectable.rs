//! `Selectable<AnyGraphqlType, GraphqlCondition>` for `GraphqlSelect`.
//!
//! Same shape as `vantage-mongodb`'s impl — most methods are direct
//! field accessors; the as_field/as_count/as_sum/as_max/as_min
//! aggregate constructors return placeholder expressions because the
//! actual aggregate semantics depend on the schema (Hasura ships
//! `<table>_aggregate { aggregate { count, sum {...} } }`; generic
//! schemas vary).

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
        // GraphQL selection sets don't accept arbitrary expressions —
        // only field names (plus arguments and aliases, which we
        // handle elsewhere). Drop silently to mirror Mongo's posture.
    }

    fn add_where_condition(&mut self, condition: impl Into<GraphqlCondition>) {
        self.conditions.push(condition.into());
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }

    fn add_order_by(&mut self, order: impl Into<GraphqlCondition>, direction: Order) {
        // Convention: an order argument that resolves to a Field
        // condition carries the column name as `field`. Anything else
        // gets dropped — same posture as Mongo's `add_order_by`.
        let cond = order.into();
        if let GraphqlCondition::Field(fc) = cond {
            self.sort.push((fc.field, direction));
        }
    }

    fn add_group_by(&mut self, expression: impl Expressive<AnyGraphqlType>) {
        self.group_by.push(expression.preview());
    }

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

    fn clear_group_by(&mut self) {
        self.group_by.clear();
    }

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
        !self.group_by.is_empty()
    }

    fn is_distinct(&self) -> bool {
        self.distinct
    }

    fn get_limit(&self) -> Option<i64> {
        self.limit
    }

    fn get_skip(&self) -> Option<i64> {
        self.skip
    }

    fn as_field(&self, _field: impl Into<String>) -> Expression<AnyGraphqlType> {
        // Subquery-as-field requires schema-driven aggregate field
        // names that aren't available at this layer; defer to a real
        // path in Phase 6 / Phase 5 once the schema map exists.
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
