// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! Commonly used traits, types and macros, for `use vantage_types::prelude::*;`.

pub use crate::null::InvariantValue;
pub use crate::record::{IntoRecord, Record, TryFromRecord, TryIntoRecord};
pub use crate::terminal_render::{RichText, Span, Style, TerminalRender};

pub use crate::vantage_type_system;

pub use vantage_types_entity::entity;

#[cfg(feature = "serde")]
pub use crate::Entity;

pub use crate::EmptyEntity;
