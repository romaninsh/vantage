// src/dataset/error.rs

use thiserror::Error;

/// Error type for dataset operations
#[derive(Error, Debug)]
pub enum DataSetError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No data available")]
    NoData,

    #[error("Capability {method} is not implemented in generic {type_name}")]
    NoCapability { method: String, type_name: String },

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

    /// Create a "capability not implemented" error with method and type information
    pub fn no_capability<S: Into<String>>(method: S, type_name: S) -> Self {
        Self::NoCapability {
            method: method.into(),
            type_name: type_name.into(),
        }
    }
}

/// Type alias for Result with DataSetError
pub type Result<T> = std::result::Result<T, DataSetError>;
