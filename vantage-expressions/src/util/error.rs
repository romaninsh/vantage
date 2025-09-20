use thiserror::Error;

#[derive(Debug)]
pub struct Error {
    context: Option<String>,
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
