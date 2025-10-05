//! Error handling utilities for the Vantage framework
//!
//! This module provides a unified error handling system using `thiserror` with
//! context support and macros for ergonomic error handling.

use thiserror::Error;

/// Error wrapper with context support
#[derive(Debug)]
pub struct VantageError {
    pub(crate) context: Option<String>,
    error: VantageErrorKind,
}

/// Core error types for the Vantage framework
#[derive(Error, Debug)]
pub enum VantageErrorKind {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No data available")]
    NoData,

    #[error("Capability {method} is not implemented in generic {type_name}")]
    NoCapability { method: String, type_name: String },

    #[error("Other error: {0}")]
    Other(String),
}

impl std::fmt::Display for VantageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)?;

        if let Some(context) = &self.context {
            write!(f, ": {}", context)?;
        }

        Ok(())
    }
}

impl std::error::Error for VantageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.error.source()
    }
}

impl VantageError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            context: None,
            error: VantageErrorKind::Other(message.into()),
        }
    }

    pub fn with_context(message: impl Into<String>, context: impl Into<String>) -> Self {
        Self {
            context: Some(context.into()),
            error: VantageErrorKind::Other(message.into()),
        }
    }

    /// Create a "no data available" error
    pub fn no_data() -> Self {
        Self {
            context: None,
            error: VantageErrorKind::NoData,
        }
    }

    /// Create a "capability not implemented" error with method and type information
    pub fn no_capability(method: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            context: None,
            error: VantageErrorKind::NoCapability {
                method: method.into(),
                type_name: type_name.into(),
            },
        }
    }

    /// Create a generic error with a message
    pub fn other(message: impl Into<String>) -> Self {
        Self {
            context: None,
            error: VantageErrorKind::Other(message.into()),
        }
    }
}

impl From<std::io::Error> for VantageError {
    fn from(err: std::io::Error) -> Self {
        Self {
            context: None,
            error: VantageErrorKind::Io(err),
        }
    }
}

/// Result type alias for Vantage operations
pub type Result<T> = std::result::Result<T, VantageError>;

/// Context trait for adding error context
pub trait Context<T> {
    fn context(self, msg: impl Into<String>) -> Result<T>;
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String;
}

impl<T, E> Context<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn context(self, msg: impl Into<String>) -> Result<T> {
        self.map_err(|err| {
            let mut error = VantageError::new(format!("{}", err));
            error.context = Some(msg.into());
            error
        })
    }

    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|err| {
            let mut error = VantageError::new(format!("{}", err));
            error.context = Some(f());
            error
        })
    }
}

/// Macro for creating VantageError instances
#[macro_export]
macro_rules! vantage_error {
    ($msg:literal $(,)?) => {
        $crate::VantageError::new($msg)
    };
    ($err:expr $(,)?) => {
        $crate::VantageError::new($err)
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::VantageError::new(format!($fmt, $($arg)*))
    };
}

pub use vantage_error;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_error_creation() {
        let err = VantageError::new("Connection failed");
        assert_eq!(err.to_string(), "Other error: Connection failed");
    }

    #[test]
    fn test_no_data_error() {
        let err = VantageError::no_data();
        assert_eq!(err.to_string(), "No data available");
    }

    #[test]
    fn test_no_capability_error() {
        let err = VantageError::no_capability("insert", "ReadOnlyDataSet");
        assert_eq!(
            err.to_string(),
            "Capability insert is not implemented in generic ReadOnlyDataSet"
        );
    }

    #[test]
    fn test_error_with_context() {
        let err = VantageError::with_context("File not found", "Failed to read config");
        let error_msg = err.to_string();
        assert!(error_msg.contains("Other error: File not found"));
        assert!(error_msg.contains("Failed to read config"));
    }

    #[test]
    fn test_context_trait() {
        use super::Context;

        fn failing_function() -> std::io::Result<String> {
            Err(io::Error::new(io::ErrorKind::NotFound, "File not found"))
        }

        let result = failing_function().context("Failed to read file");
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("File not found"));
        assert!(error_msg.contains("Failed to read file"));
    }

    #[test]
    fn test_macro() {
        let err = vantage_error!("Test error: {}", 42);
        assert_eq!(err.to_string(), "Other error: Test error: 42");
    }

    #[test]
    fn test_io_error_conversion() {
        use std::io;
        let io_err = io::Error::new(io::ErrorKind::NotFound, "File not found");
        let vantage_err = VantageError::from(io_err);
        assert_eq!(vantage_err.to_string(), "IO error: File not found");
    }
}
