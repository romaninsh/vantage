#![doc = include_str!("../README.md")]

pub mod any_expression;
pub mod capabilities;
pub mod column;
pub mod factory;
pub mod impls;
pub mod metadata;
pub mod mocks;
pub mod reference;
pub mod source;
pub mod vista;

pub use any_expression::{AnyExpression, ExpressionLike};
pub use capabilities::{PaginateKind, VistaCapabilities};
pub use column::Column;
pub use factory::VistaFactory;
pub use metadata::VistaMetadata;
pub use reference::{Reference, ReferenceKind};
pub use source::VistaSource;
pub use vista::Vista;

/// Convenience alias for the carrier type used at the `VistaSource` boundary.
pub type CborValue = ciborium::Value;
