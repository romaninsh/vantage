//! Prelude module for vantage-expressions
//!
//! This module re-exports commonly used types and traits to simplify imports.
//! Import this to get access to the most frequently used items in one go:
//!
//! ```rust
//! use vantage_expressions::prelude::*;
//! ```

// Core types
pub use crate::Expression;

// Type-erased expressions
pub use crate::{AnyExpression, ExpressionLike};

// Query source traits
pub use crate::{ExprDataSource, SelectableDataSource};

// Essential traits
pub use crate::traits::expressive::{DeferredFn, ExpressiveEnum};
pub use crate::traits::selectable::Selectable;

// Expression creation macros
pub use crate::{expr, expr_any, expr_as};

// Expression mapping and flattening
pub use crate::{ExpressionFlattener, ExpressionMap, Flatten};
