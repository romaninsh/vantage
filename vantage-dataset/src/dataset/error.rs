// src/dataset/error.rs

use thiserror::Error;

/// Error type for dataset operations
#[derive(Error, Debug)]
pub enum DataSetError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No data available")]
    NoData,

    #[error("Capability unavailable")]
    NoCapability,

    #[error("{0}")]
    Other(String),
}

impl DataSetError {
    /// Create a generic error with a message
    pub fn other<S: Into<String>>(msg: S) -> Self {
        Self::Other(msg.into())
    }

    /// Create a "no data available" error
    pub fn no_data() -> Self {
        Self::NoData
    }
}

/// Type alias for Result with DataSetError
pub type Result<T> = std::result::Result<T, DataSetError>;
