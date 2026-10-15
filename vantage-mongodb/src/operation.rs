//! Operators that build [`MongoCondition`] filters from columns: `price.gt(100)`
//! is `{ price: { "$gt": 100 } }`.
//!
//! [`MongoOperation`] is blanket-implemented for every `Expressive<T>`. The
//! field name is the expression's template, so it fits columns and plain
//! identifiers. A condition is negated with `eq_bool(false)` (or
//! `ne_bool(true)`) on [`MongoCondition`].

use bson::{Bson, doc};
use vantage_expressions::Expressive;

use crate::condition::MongoCondition;
use crate::types::{AnyMongoType, MongoType};

fn field_name<T>(expr: &(impl Expressive<T> + ?Sized)) -> String {
    expr.expr().template.clone()
}

fn to_bson_val(value: impl Into<AnyMongoType>) -> Bson {
    let any: AnyMongoType = value.into();
    any.to_bson()
}

/// Wraps each field of a document in `$not`. A deferred condition passes
/// through unchanged.
fn negate(cond: MongoCondition) -> MongoCondition {
    match cond {
        MongoCondition::Doc(doc) => {
            let mut negated = bson::Document::new();
            for (key, val) in doc {
                match val {
                    Bson::Document(inner) => {
                        negated.insert(key, doc! { "$not": inner });
                    }
                    other => {
                        negated.insert(key, doc! { "$not": { "$eq": other } });
                    }
                }
            }
            MongoCondition::Doc(negated)
        }
        MongoCondition::And(conditions) => {
            MongoCondition::And(conditions.into_iter().map(negate).collect())
        }
        // A deferred condition has no document to wrap until it resolves.
        other => other,
    }
}

/// Operators that produce a [`MongoCondition`]. Import this instead of
/// `vantage_table::operation::Operation` when working with MongoDB.
pub trait MongoOperation<T>: Expressive<T> {
    /// `{ field: { "$eq": value } }`
    fn eq(&self, value: impl Into<AnyMongoType>) -> MongoCondition
    where
        Self: Sized,
    {
        MongoCondition::Doc(doc! { field_name(self): { "$eq": to_bson_val(value) } })
    }

    /// `{ field: { "$ne": value } }`
    fn ne(&self, value: impl Into<AnyMongoType>) -> MongoCondition
    where
        Self: Sized,
    {
        MongoCondition::Doc(doc! { field_name(self): { "$ne": to_bson_val(value) } })
    }

    /// `{ field: { "$gt": value } }`
    fn gt(&self, value: impl Into<AnyMongoType>) -> MongoCondition
    where
        Self: Sized,
    {
        MongoCondition::Doc(doc! { field_name(self): { "$gt": to_bson_val(value) } })
    }

    /// `{ field: { "$gte": value } }`
    fn gte(&self, value: impl Into<AnyMongoType>) -> MongoCondition
    where
        Self: Sized,
    {
        MongoCondition::Doc(doc! { field_name(self): { "$gte": to_bson_val(value) } })
    }

    /// `{ field: { "$lt": value } }`
    fn lt(&self, value: impl Into<AnyMongoType>) -> MongoCondition
    where
        Self: Sized,
    {
        MongoCondition::Doc(doc! { field_name(self): { "$lt": to_bson_val(value) } })
    }

    /// `{ field: { "$lte": value } }`
    fn lte(&self, value: impl Into<AnyMongoType>) -> MongoCondition
    where
        Self: Sized,
    {
        MongoCondition::Doc(doc! { field_name(self): { "$lte": to_bson_val(value) } })
    }

    /// `{ field: { "$in": [values...] } }`
    fn in_<I, V>(&self, values: I) -> MongoCondition
    where
        Self: Sized,
        I: IntoIterator<Item = V>,
        V: Into<AnyMongoType>,
    {
        let arr: Vec<Bson> = values.into_iter().map(to_bson_val).collect();
        MongoCondition::Doc(doc! { field_name(self): { "$in": arr } })
    }

    /// `{ field: null }`: matches documents where the field is null or missing.
    fn is_null(&self) -> MongoCondition
    where
        Self: Sized,
    {
        MongoCondition::Doc(doc! { field_name(self): Bson::Null })
    }

    /// `{ field: { "$ne": null } }`: matches documents where the field exists and is non-null.
    fn is_not_null(&self) -> MongoCondition
    where
        Self: Sized,
    {
        MongoCondition::Doc(doc! { field_name(self): { "$ne": Bson::Null } })
    }
}

impl<T, S: Expressive<T>> MongoOperation<T> for S {}

// A condition is not an expression; this impl only satisfies the blanket
// bound so a chained operator call type-checks.
impl Expressive<AnyMongoType> for MongoCondition {
    fn expr(&self) -> vantage_expressions::Expression<AnyMongoType> {
        vantage_expressions::Expression::new(format!("{:?}", self), vec![])
    }
}

impl MongoCondition {
    /// `eq_bool(false)` negates the condition; `eq_bool(true)` keeps it.
    pub fn eq_bool(&self, value: bool) -> MongoCondition {
        if value {
            self.clone()
        } else {
            negate(self.clone())
        }
    }

    /// `ne_bool(true)` negates the condition; `ne_bool(false)` keeps it.
    pub fn ne_bool(&self, value: bool) -> MongoCondition {
        if value {
            negate(self.clone())
        } else {
            self.clone()
        }
    }
}
