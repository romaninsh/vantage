//! [`VistaResolver`]: the one way to turn a name into a [`Vista`].

use vantage_core::Result;

use crate::vista::Vista;

/// Builds the [`Vista`] a name stands for: a model in a
/// [`Catalog`](crate::Catalog), a relation's bare target, a table a script
/// names.
///
/// Each call returns a fresh, un-narrowed Vista the caller owns. A name the
/// resolver doesn't know is an error marked
/// [`not_found`](vantage_core::VantageError::is_not_found); any other failure
/// (a backend that can't be reached, a model that doesn't build) keeps its
/// own kind, so a caller can tell a missing name from an unreachable one.
///
/// Any `Fn(&str) -> Result<Vista>` closure is a resolver. A host keeps one
/// as `Arc<dyn VistaResolver>`.
pub trait VistaResolver: Send + Sync {
    fn vista(&self, name: &str) -> Result<Vista>;

    /// A Vista already in hand, as this resolver would hand it out. Identity by
    /// default; a transaction's resolver wraps it.
    fn adopt(&self, vista: Vista) -> Vista {
        vista
    }
}

impl<F: Fn(&str) -> Result<Vista> + Send + Sync> VistaResolver for F {
    fn vista(&self, name: &str) -> Result<Vista> {
        self(name)
    }
}
