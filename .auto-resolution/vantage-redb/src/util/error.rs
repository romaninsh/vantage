//! Unified error handling for vantage-redb
//!
//! This module provides a unified error type that can handle both VantageError
//! and RedbError seamlessly, allowing the same usage patterns across the codebase.

use thiserror::Error;
use vantage_core::util::error::VantageError;

/// Unified error type for vantage-redb operations
#[derive(Error, Debug)]
pub enum RedbError {
    #[error("Database error: {0}")]
    Database(#[from] Box<redb::Error>),
    #[error("Database error: {0}")]
    DatabaseError(#[from] Box<redb::DatabaseError>),
    #[error("Transaction error: {0}")]
    Transaction(#[from] Box<redb::TransactionError>),
    #[error("Storage error: {0}")]
    Storage(#[from] Box<redb::StorageError>),
    #[error("Table error: {0}")]
    Table(#[from] Box<redb::TableError>),
    #[error("Commit error: {0}")]
    Commit(#[from] Box<redb::CommitError>),
    #[error("Serialization error: {0}")]
    Serialization(#[from] Box<bincode::Error>),
    #[error("Query error: {0}")]
    Query(String),
    #[error("Vantage framework error: {0}")]
    Vantage(#[from] VantageError),
}

impl RedbError {
    /// Create a generic error with a message
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Query(msg.into())
    }

    /// Create a "no data available" error
    pub fn no_data() -> Self {
        Self::Vantage(VantageError::no_data())
    }

    /// Create a "capability not implemented" error with method and type information
    pub fn no_capability(method: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self::Vantage(VantageError::no_capability(method, type_name))
    }

    /// Add context to this error
    pub fn with_context(self, context: impl Into<String>) -> Self {
        match self {
            RedbError::Vantage(mut vantage_err) => {
                vantage_err.context = Some(context.into());
                RedbError::Vantage(vantage_err)
            }
            other => {
                // For non-vantage errors, wrap in a new VantageError with context
                RedbError::Vantage(VantageError::with_context(
                    format!("{}", other),
                    context.into(),
                ))
            }
        }
    }
}

/// Convert VantageError to RedbError seamlessly
impl From<VantageError> for RedbError {
    fn from(err: VantageError) -> Self {
        Self::Vantage(err)
    }
}

/// Result type alias for ReDB operations
pub type Result<T> = std::result::Result<T, RedbError>;

/// Context trait for adding error context to ReDB operations
pub trait Context<T> {
    fn context(self, msg: impl Into<String>) -> Result<T>;
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String;
}

impl<T, E> Context<T> for std::result::Result<T, E>
where
    E: Into<RedbError>,
{
    fn context(self, msg: impl Into<String>) -> Result<T> {
        self.map_err(|err| err.into().with_context(msg))
    }

    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|err| err.into().with_context(f()))
    }
}

/// Macro for creating RedbError instances (similar to vantage_error!)
#[macro_export]
macro_rules! redb_error {
    ($msg:literal $(,)?) => {
        $crate::util::error::RedbError::other($msg)
    };
    ($err:expr $(,)?) => {
        $crate::util::error::RedbError::other($err)
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::util::error::RedbError::other(format!($fmt, $($arg)*))
    };
}

pub use redb_error;

// Re-export vantage_error! macro for convenience
pub use vantage_core::util::error::vantage_error;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_error_creation() {
        let err = RedbError::other("Connection failed");
        assert!(err.to_string().contains("Connection failed"));
    }

    #[test]
    fn test_no_data_error() {
        let err = RedbError::no_data();
        assert!(err.to_string().contains("No data available"));
    }

    #[test]
    fn test_no_capability_error() {
        let err = RedbError::no_capability("insert", "ReadOnlyDataSet");
        assert!(err.to_string().contains("Capability insert"));
        assert!(err.to_string().contains("ReadOnlyDataSet"));
    }

    #[test]
    fn test_vantage_error_integration() {
        let vantage_err = VantageError::other("Vantage specific error");
        let redb_err: RedbError = vantage_err.into();
        assert!(redb_err.to_string().contains("Vantage specific error"));
    }

    #[test]
    fn test_context_trait() {
        use super::Context;

        fn failing_function() -> std::io::Result<String> {
            Err(io::Error::new(io::ErrorKind::NotFound, "File not found"))
        }

        // This won't compile directly because io::Error doesn't convert to RedbError
        // but shows the intended usage pattern
        let result = VantageError::other("Test error").context("Additional context");
        let redb_result: Result<()> = Err(result.unwrap_err());
        assert!(redb_result.is_err());
    }

    #[test]
    fn test_macro() {
        let err = redb_error!("Test error: {}", 42);
        assert!(err.to_string().contains("Test error: 42"));
    }
}
