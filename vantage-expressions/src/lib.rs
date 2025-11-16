#![doc = include_str!("../README.md")]

pub mod traits;

pub mod any_expression;
pub mod expression;
pub mod mocks;
pub mod prelude;
pub mod util;
pub mod value;

// pub use expression::lazy::LazyExpression;
pub use any_expression::{AnyExpression, ExpressionLike};
pub use expression::expression::Expression;
pub use expression::flatten::{ExpressionFlattener, Flatten};
pub use expression::mapping::{ExpressionMap, ExpressionMapper};
pub use traits::associated_queryable::AssociatedQueryable;
pub use traits::datasource::QuerySource;
pub use traits::datasource::SelectSource;
pub use traits::expressive::{DeferredFn, Expressive, ExpressiveEnum};
pub use traits::selectable::Selectable;
pub use vantage_core::Entity;

pub use traits::result;

/// Short type alias for `ExpressiveEnum<serde_json::Value>`
pub type Expr = ExpressiveEnum<serde_json::Value>;
