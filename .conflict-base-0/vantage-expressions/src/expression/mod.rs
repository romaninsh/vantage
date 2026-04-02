//! SQL-injection-safe expression building with templates and parameters.
//!
//! Modules:
//! - [`expression`] - Core `Expression<T>` struct
//! - [`macros`] - `expr!` and `expr_as!` macros
//! - [`mapping`] - Type conversion utilities
//! - [`flatten`] - Expression flattening
//!
//! ## Expression
//!
//! The main `Expression<T>` struct stores a template string and typed parameters.
//! ```rust
//! use vantage_expressions::prelude::*;
//!
//! let expr = Expression::new("SELECT * WHERE id = {}", vec![ExpressiveEnum::Scalar(42)]);
//! ```
//!
//! ## Macros
//!
//! Convenient macros for creating expressions with automatic type inference.
//! ```rust
//! use vantage_expressions::prelude::*;
//!
//! let query = expr!("SELECT * FROM users WHERE age > {}", 21);
//! let typed_query = expr_as!(String, "name = {}", "John");
//! let any_query: Expression<i32> = expr_any!("id = {}", 42);
//! ```
//!
//! ## Mapping
//!
//! Convert expressions between parameter types for cross-database compatibility.
//! ```rust
//! use vantage_expressions::prelude::*;
//! use serde_json::Value;
//!
//! let string_expr: Expression<String> = expr_as!(String, "name = {}", "John");
//! let value_expr: Expression<Value> = string_expr.map();
//! // Now use mapped expression as parameter in different type
//! let query: Expression<Value> = expr!("SELECT * FROM users WHERE {}", (value_expr));
//! ```
//!
//! ## Flatten
//!
//! Resolve nested expressions into flat templates with combined parameters.
//! ```rust
//! use vantage_expressions::prelude::*;
//!
//! let where_clause = expr!("age > {} AND status = {}", 21, "active");
//! let query = expr!("SELECT * FROM users WHERE {}", (where_clause));
//! let flattener = ExpressionFlattener::new();
//! let flattened = flattener.flatten(&query);
//! // Template becomes: "SELECT * FROM users WHERE age > {} AND status = {}"
//! ```

pub mod core;
pub mod flatten;
pub mod macros;
pub mod mapping;
