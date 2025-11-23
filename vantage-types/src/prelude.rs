//! Prelude module for vantage-types
//!
//! This module re-exports commonly used traits and types for convenient importing.
//!
//! # Example
//! ```rust
//! use vantage_types::prelude::*;
//! ```

// Re-export core types
pub use crate::record::{IntoRecord, Record, TryFromRecord};

// Re-export macros
pub use crate::vantage_type_system;

// Re-export proc-macros
pub use vantage_types_entity::entity;

#[cfg(feature = "serde")]
pub use crate::Entity;
