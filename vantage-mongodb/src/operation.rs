//! MongoDB-specific operation trait for building `MongoCondition` from typed columns.
//!
//! `MongoOperation<T>` is an extension trait blanket-implemented for all `Expressive<T>`
//! types (including `Column<T>`, `Expression<T>`, scalars, etc). It produces
//! `MongoCondition` (BSON documents) instead of `Expression<T>`.
//!
//! The field name is extracted from `self.expr().template` — works for simple
//! column/identifier expressions. Complex left-hand expressions will produce
//! the template string as the field name (MongoDB will treat it as a dotted path
//! or literal field name).
//!
//! # Examples
//!
//! ```ignore
//! use vantage_mongodb::operation::MongoOperation;
//! use vantage_table::column::core::Column;
//!
//! let price = Column::<i64>::new("price");
//! let cond = price.gt(100i64);
//! // => MongoCondition::Doc(doc! { "price": { "$gt": 100 } })
//!
//! // Chaining works — MongoCondition implements Expressive<AnyMongoType>
//! let cond = price.gt(10i64).eq(false);
//! // => MongoCondition::Doc(doc! { "price": { "$not": { "$gt": 10 } } })
//! ```

use bson::{Bson, doc};
use vantage_expressions::Expressive;

use crate::condition::MongoCondition;
use crate::types::{AnyMongoType, MongoType};

/// Extract the field name from an `Expressive<T>` value.
///
/// For `Column<T>` this returns the column name (e.g. `"price"`).
/// For complex expressions it returns the rendered template.
fn field_name<T>(expr: &(impl Expressive<T> + ?Sized)) -> String {
    expr.expr().template.clone()
}

/// Convert a value to `Bson` via `Into<AnyMongoType>` → `MongoType::to_bson()`.
fn to_bson_val(value: impl Into<AnyMongoType>) -> Bson {
    let any: AnyMongoType = value.into();
    any.to_bson()
}

/// Negate a `MongoCondition` by wrapping each field condition with `$not`.
fn negate(cond: MongoCondition) -> MongoCondition {
    match cond {
        MongoCondition::Doc(doc) => {
            let mut negated = bson::Document::new();
            for (key, val) in doc {
                match val {
                    // { field: { "$op": v } } → { field: { "$not": { "$op": v } } }
                    Bson::Document(inner) => {
                        negated.insert(key, doc! { "$not": inner });
                    }
                    // { field: v } → { field: { "$not": { "$eq": v } } }
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
        // Deferred can't be negated statically — pass through
        other => other,
    }
}

/// MongoDB-specific operations that produce `MongoCondition`.
///
/// Blanket-implemented for all `Expressive<T>` where values convert
/// via `Into<AnyMongoType>`. Import this trait instead of
/// `vantage_table::operation::Operation` when working with MongoDB.
pub trait MongoOperation<T>: Expressive<T> {
    /// `{ field: { "$eq": value } }`
    ///
    /// When called on a `MongoCondition`: `.eq(false)` negates, `.eq(true)` is identity.
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
}

/// Blanket: any `Expressive<T>` gets `MongoOperation<T>` for free.
impl<T, S: Expressive<T>> MongoOperation<T> for S {}

// ── MongoCondition chaining ──────────────────────────────────────────
//
// MongoCondition implements Expressive<AnyMongoType> so the blanket above
// gives it MongoOperation<AnyMongoType>. We override the default methods
// to handle boolean logic on conditions rather than building field docs.

impl Expressive<AnyMongoType> for MongoCondition {
    fn expr(&self) -> vantage_expressions::Expression<AnyMongoType> {
        // MongoCondition isn't really an expression — this is a bridge
        // so the blanket trait bound is satisfied.
        vantage_expressions::Expression::new(format!("{:?}", self), vec![])
    }
}

impl MongoCondition {
    /// `.eq(false)` negates the condition; `.eq(true)` is identity.
    /// This is the MongoCondition-aware version that overrides the blanket.
    pub fn eq_bool(&self, value: bool) -> MongoCondition {
        if value {
            self.clone()
        } else {
            negate(self.clone())
        }
    }

    /// `.ne(false)` is identity; `.ne(true)` negates.
    pub fn ne_bool(&self, value: bool) -> MongoCondition {
        if value {
            negate(self.clone())
        } else {
            self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_table::column::core::Column;

    #[test]
    fn test_column_eq() {
        let name = Column::<String>::new("name");
        let cond = name.eq("Alice");
        match cond {
            MongoCondition::Doc(doc) => {
                assert_eq!(doc, doc! { "name": { "$eq": "Alice" } });
            }
            _ => panic!("expected Doc"),
        }
    }

    #[test]
    fn test_column_gt() {
        let price = Column::<i64>::new("price");
        let cond = price.gt(100i64);
        match cond {
            MongoCondition::Doc(doc) => {
                assert_eq!(doc, doc! { "price": { "$gt": 100i64 } });
            }
            _ => panic!("expected Doc"),
        }
    }

    #[test]
    fn test_column_in() {
        let status = Column::<String>::new("status");
        let cond = status.in_(vec!["active", "pending"]);
        match cond {
            MongoCondition::Doc(doc) => {
                assert_eq!(doc, doc! { "status": { "$in": ["active", "pending"] } });
            }
            _ => panic!("expected Doc"),
        }
    }

    #[test]
    fn test_chaining_gt_eq_false() {
        let price = Column::<i64>::new("price");
        // price.gt(10).eq_bool(false) means "NOT price > 10"
        let cond = price.gt(10i64).eq_bool(false);
        match cond {
            MongoCondition::Doc(doc) => {
                assert_eq!(doc, doc! { "price": { "$not": { "$gt": 10i64 } } });
            }
            _ => panic!("expected Doc"),
        }
    }

    #[test]
    fn test_chaining_gt_eq_true() {
        let price = Column::<i64>::new("price");
        // price.gt(10).eq_bool(true) is identity
        let cond = price.gt(10i64).eq_bool(true);
        match cond {
            MongoCondition::Doc(doc) => {
                assert_eq!(doc, doc! { "price": { "$gt": 10i64 } });
            }
            _ => panic!("expected Doc"),
        }
    }

    #[test]
    fn test_negate_simple_value() {
        let cond = MongoCondition::Doc(doc! { "active": true });
        let negated = negate(cond);
        match negated {
            MongoCondition::Doc(doc) => {
                assert_eq!(doc, doc! { "active": { "$not": { "$eq": true } } });
            }
            _ => panic!("expected Doc"),
        }
    }

    #[test]
    fn test_negate_operator() {
        let cond = MongoCondition::Doc(doc! { "price": { "$gt": 100 } });
        let negated = negate(cond);
        match negated {
            MongoCondition::Doc(doc) => {
                assert_eq!(doc, doc! { "price": { "$not": { "$gt": 100 } } });
            }
            _ => panic!("expected Doc"),
        }
    }

    #[test]
    fn test_condition_is_correct_type() {
        let price = Column::<i64>::new("price");
        let cond: MongoCondition = price.gt(100i64);
        let _: MongoCondition = cond;
    }
}
