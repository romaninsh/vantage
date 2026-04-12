//! Selectable<AnyMongoType, MongoCondition> implementation for MongoSelect.

use vantage_expressions::traits::selectable::SourceRef;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum, Order, Selectable};

use crate::condition::MongoCondition;
use crate::select::MongoSelect;
use crate::types::AnyMongoType;

impl Selectable<AnyMongoType, MongoCondition> for MongoSelect {
    fn add_source(&mut self, source: impl Into<SourceRef<AnyMongoType>>, _alias: Option<String>) {
        let source_ref = source.into().into_expressive_enum();
        if let ExpressiveEnum::Scalar(val) = source_ref
            && let Some(name) = val.try_get::<String>()
        {
            self.collection = Some(name);
        }
    }

    fn add_field(&mut self, field: impl Into<String>) {
        self.fields.push(field.into());
    }

    fn add_expression(&mut self, _expression: impl Expressive<AnyMongoType>) {
        // MongoDB projections don't support arbitrary expressions in find().
    }

    fn add_where_condition(&mut self, condition: impl Into<MongoCondition>) {
        self.conditions.push(condition.into());
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }

    fn add_order_by(&mut self, order: impl Into<MongoCondition>, direction: Order) {
        // Convention: a MongoCondition::Doc with a single key → use that key.
        // The direction from the Order param overrides whatever value is in the doc.
        let cond = order.into();
        if let MongoCondition::Doc(doc) = &cond
            && let Some((key, _)) = doc.iter().next()
        {
            let dir = if direction.ascending { 1 } else { -1 };
            self.sort.push((key.to_string(), dir));
        }
    }

    fn add_group_by(&mut self, expression: impl Expressive<AnyMongoType>) {
        let preview = expression.preview();
        self.group_by.push(preview);
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

    fn as_count(&self) -> Expression<AnyMongoType> {
        let coll = self.collection.as_deref().unwrap_or("?");
        Expression::new(format!("db.{}.countDocuments()", coll), vec![])
    }

    fn as_sum(&self, column: impl Expressive<AnyMongoType>) -> Expression<AnyMongoType> {
        let coll = self.collection.as_deref().unwrap_or("?");
        Expression::new(
            format!("db.{}.aggregate($sum: {})", coll, column.preview()),
            vec![],
        )
    }

    fn as_max(&self, column: impl Expressive<AnyMongoType>) -> Expression<AnyMongoType> {
        let coll = self.collection.as_deref().unwrap_or("?");
        Expression::new(
            format!("db.{}.aggregate($max: {})", coll, column.preview()),
            vec![],
        )
    }

    fn as_min(&self, column: impl Expressive<AnyMongoType>) -> Expression<AnyMongoType> {
        let coll = self.collection.as_deref().unwrap_or("?");
        Expression::new(
            format!("db.{}.aggregate($min: {})", coll, column.preview()),
            vec![],
        )
    }
}
