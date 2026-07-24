//! `FlashRejection` — a structured refusal of a [`ChangeFlash`](crate::ChangeFlash).
//!
//! A write path that turns a flash down (an `on_flash` route running
//! validation, a master mapping a constraint violation) can say *which
//! fields* failed, not just that the write did. The rejection rides the
//! [`VantageError`] source chain back through the pipeline; the servo
//! extracts it with [`from_error`](FlashRejection::from_error) and
//! carries it in [`ServoStatus::Failed`](crate::ServoStatus::Failed),
//! where a form maps the entries onto its fields.
//!
//! A failure with no structure still becomes a rejection — message only,
//! empty field list — so consumers handle exactly one shape.

use std::fmt;

use vantage_core::{Context as _, VantageError};

/// A refused flash: an overall message plus zero or more
/// `(field, message)` entries naming what failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashRejection {
    message: String,
    field_errors: Vec<(String, String)>,
}

impl FlashRejection {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            field_errors: Vec::new(),
        }
    }

    /// Name a failed field (builder, chainable).
    pub fn with_field(mut self, field: impl Into<String>, message: impl Into<String>) -> Self {
        self.field_errors.push((field.into(), message.into()));
        self
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn field_errors(&self) -> &[(String, String)] {
        &self.field_errors
    }

    /// The message for one field, when the rejection names it.
    pub fn error_for(&self, field: &str) -> Option<&str> {
        self.field_errors
            .iter()
            .find(|(f, _)| f == field)
            .map(|(_, m)| m.as_str())
    }

    /// Wrap into a [`VantageError`] whose source chain carries `self` —
    /// what an `on_flash` route returns to reject a flash with structure.
    pub fn into_error(self) -> VantageError {
        let message = self.message.clone();
        Err::<(), _>(self)
            .context(VantageError::other(message))
            .unwrap_err()
    }

    /// Recover a rejection from an error's source chain. `None` when the
    /// failure carried no structure.
    pub fn from_error(err: &VantageError) -> Option<Self> {
        let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(err);
        while let Some(e) = cur {
            if let Some(rejection) = e.downcast_ref::<FlashRejection>() {
                return Some(rejection.clone());
            }
            cur = e.source();
        }
        None
    }

    /// Recover a rejection, falling back to a message-only one — the
    /// single shape [`ServoStatus::Failed`](crate::ServoStatus::Failed)
    /// carries.
    pub fn from_error_or_message(err: &VantageError) -> Self {
        Self::from_error(err).unwrap_or_else(|| Self::new(err.to_string()))
    }
}

impl fmt::Display for FlashRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if !self.field_errors.is_empty() {
            write!(f, " [")?;
            for (i, (field, msg)) in self.field_errors.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{field}: {msg}")?;
            }
            write!(f, "]")?;
        }
        Ok(())
    }
}

impl std::error::Error for FlashRejection {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_error_chain() {
        let rejection = FlashRejection::new("validation failed")
            .with_field("price", "must be positive")
            .with_field("name", "required");
        let err = rejection.clone().into_error();
        assert_eq!(FlashRejection::from_error(&err), Some(rejection));
    }

    #[test]
    fn survives_extra_wrapping() {
        let inner = FlashRejection::new("nope")
            .with_field("a", "bad")
            .into_error();
        let outer = Err::<(), _>(inner)
            .context(vantage_core::error!("route rejected the flash"))
            .unwrap_err();
        let got = FlashRejection::from_error(&outer).expect("found through wrapping");
        assert_eq!(got.error_for("a"), Some("bad"));
    }

    #[test]
    fn unstructured_failure_becomes_message_only() {
        let err = vantage_core::error!("disk on fire");
        assert_eq!(FlashRejection::from_error(&err), None);
        let fallback = FlashRejection::from_error_or_message(&err);
        assert_eq!(fallback.message(), "disk on fire");
        assert!(fallback.field_errors().is_empty());
    }
}
