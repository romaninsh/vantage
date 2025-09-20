//! # Vantage Expressions
//!
//! A database-agnostic expression framework with nesting support, extensible interfaces
//!

pub mod expression;
pub mod mocks;
pub mod protocol;
pub mod util;
pub mod value;

// pub use expression::lazy::LazyExpression;
pub use expression::flatten::{Flatten, OwnedExpressionFlattener};
pub use expression::owned::Expression;
pub use protocol::associated_queryable::AssociatedQueryable;
pub use protocol::datasource::DataSource;
pub use protocol::expressive::IntoExpressive;
pub use protocol::selectable::Selectable;

pub use protocol::result;

/// Short type alias for `IntoExpressive<OwnedExpression>`
pub type Expr = IntoExpressive<Expression>;
