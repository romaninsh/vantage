//! # Vantage Expressions
//!
//! A database-agnostic expression framework with nesting support, extensible interfaces
//!

pub mod expression;
pub mod protocol;
pub mod util;
pub mod value;

// pub use expression::lazy::LazyExpression;
pub use expression::flatten::{Flatten, OwnedExpressionFlattener};
pub use expression::owned::OwnedExpression;
pub use protocol::expressive::{DataSource, IntoExpressive};
pub use protocol::selectable::Selectable;

/// Short type alias for `IntoExpressive<OwnedExpression>`
pub type Expr = IntoExpressive<OwnedExpression>;
