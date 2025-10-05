use thiserror::Error;

#[derive(Debug)]
pub struct Error {
    pub(crate) context: Option<String>,
    error: MyError,
}

#[derive(Error, Debug)]
pub enum MyError {
    #[error("Other error: {0}")]
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)?;

        if let Some(context) = &self.context {
            context.fmt(f)?;
        }

        Ok(())
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            context: None,
            error: MyError::Other(message.into()),
        }
    }

    pub fn with_context(message: impl Into<String>, context: impl Into<String>) -> Self {
        Self {
            context: Some(context.into()),
            error: MyError::Other(message.into()),
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
macro_rules! error {
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

pub use error;
