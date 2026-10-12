#![doc = include_str!("../README.md")]

mod capabilities;
mod catalog;
mod change;
mod column;
mod computed;
mod contained;
#[cfg(feature = "contract")]
pub mod contract;
mod factory;
mod filter;
mod forward;
mod insert;
mod local;
mod metadata;
#[cfg(any(test, feature = "mocks"))]
pub mod mocks;
mod page;
mod query;
mod reference;
mod resolver;
#[cfg(feature = "rhai")]
pub mod rhai;
mod shell;
mod sort;
mod spec;
mod vista;

pub use capabilities::{Capability, VistaCapabilities, unknown_relation, unsupported};
pub use catalog::Catalog;
pub use change::{VistaChange, VistaChangeStream};
pub use column::{Column, ColumnFlag};
pub use contained::{ContainedWriteback, build_contained_vista};
pub use factory::VistaFactory;
pub use filter::{FilterOp, operand_text};
pub use metadata::VistaMetadata;
pub use page::{NativeCondition, Page, Preview};
pub use query::{Condition, VistaQuery};
pub use reference::{JoinKey, Reference, ReferenceKind};
pub use resolver::VistaResolver;
pub use shell::{CloneTableShell, TableShell};
pub use sort::SortDirection;
pub use spec::{
    ColumnSpec, ContainedYaml, NoExtras, ReferenceSpec, ReferenceSugar, SpecResolver, VistaSpec,
    resolve_base_spec,
};
pub use vista::Vista;

/// Convenience alias for the carrier type used at the `TableShell` boundary.
pub type CborValue = ciborium::Value;

/// Paths [`forward_table_shell!`] expands to, so callers need no extra deps.
#[doc(hidden)]
pub mod __private {
    pub use crate::forward::ForwardShell;
    pub use async_trait::async_trait;
}

/// What nearly every user of vantage-vista writes.
///
/// ```
/// use vantage_vista::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        Capability, CborValue, Condition, FilterOp, SortDirection, Vista, VistaQuery, VistaResolver,
    };
    pub use vantage_dataset::{InsertableValueSet, ReadableValueSet, ValueSet, WritableValueSet};
}
