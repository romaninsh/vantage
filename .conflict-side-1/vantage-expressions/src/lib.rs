//! # Vantage Expressions
//!
//! A database-agnostic expression framework with nesting support, extensible interfaces
//!

pub mod any_expression;
pub mod expression;
pub mod mocks;
pub mod prelude;
pub mod protocol;
pub mod util;
pub mod value;

// pub use expression::lazy::LazyExpression;
pub use any_expression::{AnyExpression, ExpressionLike};
pub use expression::flatten::{ExpressionFlattener, Flatten};
pub use expression::owned::Expression;
pub use protocol::associated_queryable::AssociatedQueryable;
pub use protocol::datasource::QuerySource;
pub use protocol::datasource::SelectSource;
pub use protocol::expressive::{DeferredFn, ExpressiveEnum};
pub use protocol::queryable::Queryable;
pub use protocol::selectable::Selectable;
pub use vantage_core::Entity;

pub use protocol::result;

/// Short type alias for `ExpressiveEnum<serde_json::Value>`
pub type Expr = ExpressiveEnum<serde_json::Value>;
