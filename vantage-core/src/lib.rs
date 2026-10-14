// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! # Vantage Core
//!
//! Error type, request priority and small helpers shared by every Vantage crate.

pub mod request;
pub mod util;
pub use request::Priority;
pub use util::{Context, ErrorKind, IntoVec, Result, VantageError};
