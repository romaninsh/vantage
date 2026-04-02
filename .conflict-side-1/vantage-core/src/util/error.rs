//! Error handling utilities for the Vantage framework
//!
//! This module provides a unified error handling system using `thiserror` with
//! context support and macros for ergonomic error handling.

use indexmap::IndexMap;
use std::fmt;

/// VantageError with location tracking and context information
#[derive(Debug)]
pub struct VantageError {
    message: String,
    location: Option<String>,
    pub context: Box<IndexMap<String, String>>,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl fmt::Display for VantageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Simple inline format for tests and programmatic use
        write!(f, "{}", self.message)?;

        // Add context if present
        if !self.context.is_empty() {
            write!(f, " (")?;
            for (i, (key, value)) in self.context.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}: {}", key, value)?;
            }
            write!(f, ")")?;
        }

        // Add source if present
        if let Some(source) = &self.source {
            write!(f, ": {}", source)?;
        }

        Ok(())
    }
}

impl VantageError {
    fn fmt_with_indent(
        &self,
        f: &mut fmt::Formatter<'_>,
        indent: &str,
        use_color: bool,
    ) -> fmt::Result {
        // Error message
        if indent.is_empty() {
            // Top level
            if use_color {
                #[cfg(feature = "colored-errors")]
                {
                    use owo_colors::OwoColorize;
                    writeln!(f, "{} {}", "Error:".red().bold(), self.message.red())?;
                }
                #[cfg(not(feature = "colored-errors"))]
                writeln!(f, "Error: {}", self.message)?;
            } else {
                writeln!(f, "Error: {}", self.message)?;
            }
        } else {
            // Nested level - remove 4 spaces from indent for the arrow line
            let arrow_indent = if indent.len() >= 4 {
                &indent[..indent.len() - 4]
            } else {
                ""
            };

            if use_color {
                #[cfg(feature = "colored-errors")]
                {
                    use owo_colors::OwoColorize;
                    writeln!(
                        f,
                        "{}{}─▶ {}",
                        arrow_indent,
                        "╰".bright_black(),
                        self.message.red()
                    )?;
                }
                #[cfg(not(feature = "colored-errors"))]
                writeln!(f, "{}╰─▶ {}", arrow_indent, self.message)?;
            } else {
                writeln!(f, "{}╰─▶ {}", arrow_indent, self.message)?;
            }
        }

        // Determine if there are more items below (source errors)
        let has_source = self.source.is_some();

        // Location
        if let Some(location) = &self.location {
            let is_last = !has_source && self.context.is_empty();
            let prefix = if is_last { "╰╴" } else { "├╴" };

            if use_color {
                #[cfg(feature = "colored-errors")]
                {
                    use owo_colors::OwoColorize;
                    writeln!(
                        f,
                        "{}{}at {}",
                        indent,
                        prefix.bright_black(),
                        location.cyan()
                    )?;
                }
                #[cfg(not(feature = "colored-errors"))]
                writeln!(f, "{}{}at {}", indent, prefix, location)?;
            } else {
                writeln!(f, "{}{}at {}", indent, prefix, location)?;
            }
        }

        // Context info
        let context_count = self.context.len();
        for (idx, (key, value)) in self.context.iter().enumerate() {
            let is_last = !has_source && idx == context_count - 1;
            let prefix = if is_last { "╰╴" } else { "├╴" };

            if use_color {
                #[cfg(feature = "colored-errors")]
                {
                    use owo_colors::OwoColorize;
                    writeln!(
                        f,
                        "{}{}{}: {}",
                        indent,
                        prefix.bright_black(),
                        key.yellow(),
                        value.white()
                    )?;
                }
                #[cfg(not(feature = "colored-errors"))]
                writeln!(f, "{}{}{}: {}", indent, prefix, key, value)?;
            } else {
                writeln!(f, "{}{}{}: {}", indent, prefix, key, value)?;
            }
        }

        // Source error chain
        if let Some(source) = &self.source {
            if use_color {
                #[cfg(feature = "colored-errors")]
                {
                    use owo_colors::OwoColorize;
                    writeln!(f, "{}{}", indent, "│".bright_black())?;
                }
                #[cfg(not(feature = "colored-errors"))]
                writeln!(f, "{}│", indent)?;
            } else {
                writeln!(f, "{}│", indent)?;
            }

            if let Some(vantage_source) = source.downcast_ref::<VantageError>() {
                // Recursively format VantageError with increased indentation for details
                let new_indent = format!("{}    ", indent);
                vantage_source.fmt_with_indent(f, &new_indent, use_color)?;
            } else {
                // Non-VantageError source
                if use_color {
                    #[cfg(feature = "colored-errors")]
                    {
                        use owo_colors::OwoColorize;
                        writeln!(f, "{}{}─▶ {}", indent, "╰".bright_black(), source)?;
                    }
                    #[cfg(not(feature = "colored-errors"))]
                    writeln!(f, "{}╰─▶ {}", indent, source)?;
                } else {
                    writeln!(f, "{}╰─▶ {}", indent, source)?;
                }
            }
        }

        Ok(())
    }
}

impl std::error::Error for VantageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|e| e.as_ref() as _)
    }
}

impl VantageError {
    /// Create a new VantageError with message and location
    pub fn new(message: impl Into<String>, location: String) -> Self {
        Self {
            message: message.into(),
            location: Some(location),
            context: Box::new(IndexMap::new()),
            source: None,
        }
    }

    /// Create a "no data available" error
    pub fn no_data() -> Self {
        Self {
            message: "No data available".to_string(),
            location: None,
            context: Box::new(IndexMap::new()),
            source: None,
        }
    }

    /// Create a "capability not implemented" error with method and type information
    pub fn no_capability(method: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            message: format!(
                "Capability {} is not implemented in generic {}",
                method.into(),
                type_name.into()
            ),
            location: None,
            context: Box::new(IndexMap::new()),
            source: None,
        }
    }

    /// Create a generic error with a message (no location)
    pub fn other(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            location: None,
            context: Box::new(IndexMap::new()),
            source: None,
        }
    }
}

impl From<std::io::Error> for VantageError {
    fn from(err: std::io::Error) -> Self {
        VantageError {
            message: "IO error".to_string(),
            location: None,
            context: Box::new(IndexMap::new()),
            source: Some(Box::new(err)),
        }
    }
}

impl std::process::Termination for VantageError {
    fn report(self) -> std::process::ExitCode {
        // Use colored output when printing to stderr in report()
        #[cfg(feature = "colored-errors")]
        let use_color = atty::is(atty::Stream::Stderr);
        #[cfg(not(feature = "colored-errors"))]
        let use_color = false;

        // Format with color for stderr output
        struct ColoredError<'a>(&'a VantageError, bool);

        impl<'a> fmt::Display for ColoredError<'a> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt_with_indent(f, "", self.1)
            }
        }

        eprintln!("{}", ColoredError(&self, use_color));
        std::process::ExitCode::FAILURE
    }
}

/// Result type alias for Vantage operations
pub type Result<T> = std::result::Result<T, VantageError>;

/// Trait for adding context to Results
pub trait Context<T> {
    fn context(self, err: impl Into<VantageError>) -> Result<T>;
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> VantageError;
}

impl<T, E: std::error::Error + Send + Sync + 'static> Context<T> for std::result::Result<T, E> {
    fn context(self, err: impl Into<VantageError>) -> Result<T> {
        self.map_err(|source_err| {
            let mut vantage_err = err.into();
            // Check if source_err is VantageError
            if std::any::TypeId::of::<E>() == std::any::TypeId::of::<VantageError>() {
                // If it's already VantageError, wrap it properly
                let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(source_err);
                vantage_err.source = Some(boxed);
            } else {
                vantage_err.source = Some(Box::new(source_err));
            }
            vantage_err
        })
    }

    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> VantageError,
    {
        self.map_err(|source_err| {
            let mut vantage_err = f();
            // Check if source_err is VantageError
            if std::any::TypeId::of::<E>() == std::any::TypeId::of::<VantageError>() {
                // If it's already VantageError, wrap it properly
                let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(source_err);
                vantage_err.source = Some(boxed);
            } else {
                vantage_err.source = Some(Box::new(source_err));
            }
            vantage_err
        })
    }
}

/// From String for backward compatibility
impl From<String> for VantageError {
    fn from(msg: String) -> Self {
        VantageError {
            message: msg,
            location: None,
            context: Box::new(IndexMap::new()),
            source: None,
        }
    }
}

impl From<&str> for VantageError {
    fn from(msg: &str) -> Self {
        msg.to_string().into()
    }
}

/// Macro for creating VantageError with location and context
#[macro_export]
macro_rules! error {
    ($msg:expr $(, $key:ident = $value:expr)*) => {{
        let mut err = $crate::VantageError::new(
            $msg,
            format!("{}:{}:{}", file!(), line!(), column!())
        );
        $(
            err.context.insert(stringify!($key).to_string(), format!("{:?}", $value));
        )*
        err
    }};
}

/// Legacy macro for creating VantageError instances (simple)
#[macro_export]
macro_rules! vantage_error {
    ($msg:literal $(,)?) => {
        $crate::VantageError::other($msg)
    };
    ($err:expr $(,)?) => {
        $crate::VantageError::other($err)
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::VantageError::other(format!($fmt, $($arg)*))
    };
}

pub use {error, vantage_error};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

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
        assert_eq!(err.to_string(), "Test error: 42");
    }

    #[test]
    fn test_io_error_conversion() {
        use std::io;
        let io_err = io::Error::new(io::ErrorKind::NotFound, "File not found");
        let vantage_err = VantageError::from(io_err);
        assert_eq!(vantage_err.to_string(), "IO error: File not found");
    }
}
