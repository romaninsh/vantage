use thiserror::Error;
use vantage_core::util::error::VantageError;

#[derive(Debug)]
pub struct Error {
    pub(crate) context: Option<String>,
    error: RedbError,
}

#[derive(Error, Debug)]
pub enum RedbError {
    #[error("Bincode error: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("ReDB error: {0}")]
    Redb(#[from] Box<redb::Error>),
    #[error("ReDB transaction error: {0}")]
    RedbTransaction(#[from] Box<redb::TransactionError>),
    #[error("ReDB table error: {0}")]
    RedbTable(#[from] Box<redb::TableError>),
    #[error("ReDB storage error: {0}")]
    RedbStorage(#[from] Box<redb::StorageError>),
    #[error("ReDB core error: {0}")]
    RedbCore(#[from] Box<crate::redb::core::RedbError>),
    #[error("Vantage error: {0}")]
    Vantage(#[from] Box<VantageError>),
    #[error("Other error: {0}")]
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)?;

        if let Some(context) = &self.context {
            write!(f, " ({})", context)?;
        }

        Ok(())
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            context: None,
            error: RedbError::Other(message.into()),
        }
    }

    pub fn with_context(message: impl Into<String>, context: impl Into<String>) -> Self {
        Self {
            context: Some(context.into()),
            error: RedbError::Other(message.into()),
        }
    }

    /// Create from VantageError
    pub fn from_vantage(err: VantageError) -> Self {
        Self {
            context: None,
            error: RedbError::Vantage(Box::new(err)),
        }
    }
}

impl From<bincode::Error> for Error {
    fn from(err: bincode::Error) -> Self {
        Self {
            context: None,
            error: RedbError::Bincode(err),
        }
    }
}

impl From<redb::Error> for Error {
    fn from(err: redb::Error) -> Self {
        Self {
            context: None,
            error: RedbError::Redb(Box::new(err)),
        }
    }
}

impl From<redb::TransactionError> for Error {
    fn from(err: redb::TransactionError) -> Self {
        Self {
            context: None,
            error: RedbError::RedbTransaction(Box::new(err)),
        }
    }
}

impl From<redb::TableError> for Error {
    fn from(err: redb::TableError) -> Self {
        Self {
            context: None,
            error: RedbError::RedbTable(Box::new(err)),
        }
    }
}

impl From<redb::StorageError> for Error {
    fn from(err: redb::StorageError) -> Self {
        Self {
            context: None,
            error: RedbError::RedbStorage(Box::new(err)),
        }
    }
}

impl From<crate::redb::core::RedbError> for Error {
    fn from(err: crate::redb::core::RedbError) -> Self {
        Self {
            context: None,
            error: RedbError::RedbCore(Box::new(err)),
        }
    }
}

impl From<VantageError> for Error {
    fn from(err: VantageError) -> Self {
        Self {
            context: None,
            error: RedbError::Vantage(Box::new(err)),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Context trait for adding error context like anyhow
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
            let mut error = Error::new(format!("{}", err));
            error.context = Some(msg.into());
            error
        })
    }

    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|err| {
            let mut error = Error::new(format!("{}", err));
            error.context = Some(f());
            error
        })
    }
}

/// Macro for creating Error instances, similar to anyhow!
#[macro_export]
macro_rules! redb_error {
    ($msg:literal $(,)?) => {
        $crate::util::Error::new($msg)
    };
    ($err:expr $(,)?) => {
        $crate::util::Error::new($err)
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::util::Error::new(format!($fmt, $($arg)*))
    };
}

pub use redb_error;

/// Re-export vantage_error macro for seamless usage
pub use vantage_core::util::error::vantage_error;

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self {
            context: None,
            error: RedbError::Other(format!("IO error: {}", err)),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self {
            context: None,
            error: RedbError::Other(format!("JSON error: {}", err)),
        }
    }
}

impl From<redb::DatabaseError> for Error {
    fn from(err: redb::DatabaseError) -> Self {
        Self {
            context: None,
            error: RedbError::Other(format!("Database error: {}", err)),
        }
    }
}

impl From<redb::CommitError> for Error {
    fn from(err: redb::CommitError) -> Self {
        Self {
            context: None,
            error: RedbError::Other(format!("Commit error: {}", err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_error_handling() {
        // Test VantageError integration
        let vantage_err = VantageError::other("test vantage error");
        let redb_err: Error = vantage_err.into();
        assert!(redb_err.to_string().contains("test vantage error"));

        // Test vantage_error macro works
        let macro_err = vantage_error!("macro error: {}", 42);
        let redb_macro_err: Error = macro_err.into();
        assert!(redb_macro_err.to_string().contains("macro error: 42"));
    }
}
