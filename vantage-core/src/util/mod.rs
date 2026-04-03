//! Utility modules for vantage-core

pub mod error;
pub mod into_vec;

pub use error::{Context, Result, VantageError};
pub use into_vec::IntoVec;
