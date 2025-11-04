//! [`Expression`] and [`ExpressionArc`] structs for building SQL query templates
//!
//! There are two types of SQL expressions:
//! - [`Expression`]: A simple expression that may contain parameters of type [`serde_json::Value`].
//! - [`ExpressionArc`]: An expression that can have shared ownership of its parameters, that implement
//! trayt [`SqlChunk`]
//!
//! Parameters to the above expressions must implement [`SqlChunk`] trait.
//!
//! [`ExpressionArc`] can be converted into an [`Expression`] by calling [`ExpressionArc::render_chunk()`].
//!
//! [`SqlChunk`]: super::chunk::SqlChunk
pub mod expression;
pub mod expression2;
pub mod expression_arc;

pub use expression::Expression;
pub use expression_arc::ExpressionArc;
pub use expression_arc::WrapArc;
