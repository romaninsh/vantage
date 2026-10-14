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
}

impl<F: Fn(&str) -> Result<Vista> + Send + Sync> VistaResolver for F {
    fn vista(&self, name: &str) -> Result<Vista> {
        self(name)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use vantage_core::error;

    use super::*;
    use crate::mocks::MockShell;

    #[test]
    fn a_closure_is_a_resolver() {
        let resolver: Arc<dyn VistaResolver> = Arc::new(|name: &str| match name {
            "user" => Ok(Vista::new(name, MockShell::new())),
            _ => Err(error!("no such table", name = name).mark_not_found()),
        });
        assert_eq!(resolver.vista("user").unwrap().name(), "user");
        assert!(resolver.vista("nope").unwrap_err().is_not_found());
    }
}
