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
pub use crate::{QuerySource, SelectSource};

// Essential protocol traits
pub use crate::protocol::selectable::Selectable;

// Expression creation macro
pub use crate::expr;
