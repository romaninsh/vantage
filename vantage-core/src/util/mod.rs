// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! Utility modules for vantage-core

pub mod error;
pub mod into_vec;

pub use error::{Context, ErrorKind, Result, VantageError};
pub use into_vec::IntoVec;
