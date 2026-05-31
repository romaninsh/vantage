#![doc = include_str!("../README.md")]

pub mod any_expression;
pub mod capabilities;
pub mod column;
pub mod contained;
pub mod factory;
pub mod flags;
pub mod impls;
pub mod insert;
pub mod metadata;
pub mod mocks;
pub mod reference;
pub mod sort;
pub mod source;
pub mod spec;
pub mod vista;

pub use any_expression::{AnyExpression, ExpressionLike};
pub use capabilities::VistaCapabilities;
pub use column::Column;
pub use contained::{
    ContainedRefResolver, ContainedShell, ContainedWriteback, build_contained_vista,
};
pub use factory::VistaFactory;
pub use metadata::VistaMetadata;
pub use reference::{ContainedKind, ContainedSpec, Reference, ReferenceKind};
pub use sort::SortDirection;
pub use source::TableShell;
pub use spec::{ColumnSpec, ContainedYaml, NoExtras, ReferenceSpec, ReferenceSugar, VistaSpec};
pub use vista::Vista;

/// Convenience alias for the carrier type used at the `TableShell` boundary.
pub type CborValue = ciborium::Value;
