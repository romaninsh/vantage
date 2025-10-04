//! # Vantage Expressions
//!
//! A database-agnostic expression framework with nesting support, extensible interfaces
//!

pub mod expression;
pub mod mocks;
pub mod prelude;
pub mod protocol;
pub mod util;
pub mod value;

// pub use expression::lazy::LazyExpression;
pub use expression::flatten::{ExpressionFlattener, Flatten};
pub use expression::owned::Expression;
pub use protocol::associated_queryable::AssociatedQueryable;
pub use protocol::datasource::QuerySource;
pub use protocol::datasource::SelectSource;
pub use protocol::expressive::IntoExpressive;
pub use protocol::queryable::Queryable;
pub use protocol::selectable::Selectable;

pub use protocol::result;

/// Short type alias for `IntoExpressive<Expression>`
pub type Expr = IntoExpressive<Expression>;
