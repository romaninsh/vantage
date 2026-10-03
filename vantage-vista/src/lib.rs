#![doc = include_str!("../README.md")]

pub mod aggregate;
pub mod any_expression;
pub mod capabilities;
pub mod column;
pub mod contained;
pub mod factory;
pub mod filter;
pub mod flags;
mod forward;
pub mod impls;
pub mod insert;
pub mod metadata;
pub mod mocks;
pub mod reference;
#[cfg(feature = "rhai")]
pub mod rhai;
pub mod sort;
pub mod source;
pub mod spec;
pub mod vista;

pub use aggregate::AggregateSpec;
pub use any_expression::{AnyExpression, ExpressionLike};
pub use capabilities::VistaCapabilities;
pub use column::Column;
pub use contained::{
    ContainedRefResolver, ContainedShell, ContainedWriteback, build_contained_vista,
};
pub use factory::VistaFactory;
pub use filter::{FilterOp, operand_text};
pub use metadata::VistaMetadata;
pub use reference::{ContainedKind, ContainedSpec, Reference, ReferenceKind};
#[cfg(feature = "rhai")]
pub use rhai::{
    AugmentSourceFn, DEFAULT_LIMIT, DataVocab, Handle, LazyValueFn, MAX_LIMIT, MIN_LIMIT,
    RecordDraft, TargetResolver, Terminals, Writes, augment_source_closure, cbor_to_dynamic,
    dynamic_to_cbor, eval_augment_source, eval_lazy_expression, eval_modify_script,
    eval_ref_script, lazy_value_closure, map_to_record, preview_script, record_to_dynamic,
    record_to_map, run_script,
};
pub use sort::SortDirection;
pub use source::{TableShell, VistaChange, VistaChangeStream};
pub use spec::{
    ColumnSpec, ContainedYaml, JoinKey, NoExtras, ReferenceSpec, ReferenceSugar, VistaSpec,
};
pub use vista::Vista;

/// Convenience alias for the carrier type used at the `TableShell` boundary.
pub type CborValue = ciborium::Value;

/// Paths [`forward_table_shell!`] expands to, so callers need no extra deps.
#[doc(hidden)]
pub mod __private {
    pub use async_trait::async_trait;
    pub use indexmap::IndexMap;
    pub use serde_json;
    pub use vantage_core::Result;
    #[cfg(feature = "rhai")]
    pub use vantage_rhai;

    pub type Rec = vantage_types::Record<crate::CborValue>;
}

/// Common imports for working with vantage-vista.
///
/// ```
/// use vantage_vista::prelude::*;
/// ```
pub mod prelude {
    pub use crate::CborValue;
    pub use crate::capabilities::VistaCapabilities;
    pub use crate::factory::VistaFactory;
    pub use crate::sort::SortDirection;
    pub use crate::source::TableShell;
    pub use crate::vista::Vista;
}
