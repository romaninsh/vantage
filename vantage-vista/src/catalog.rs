//! [`Catalog`]: named models, each built on demand by its own loader.

use std::fmt;
use std::sync::Arc;

use indexmap::IndexMap;
use vantage_core::{Result, error};

use crate::{resolver::VistaResolver, vista::Vista};

/// A map from model name to the loader that builds it.
///
/// The models may live in different backends: a catalog only knows how to
/// build each one by name. It is itself a [`VistaResolver`], so it goes
/// wherever a resolver is expected. Traversing a relation across models is
/// [`Reference::narrow`](crate::Reference::narrow) over
/// `catalog.vista(&reference.target)?`.
///
/// Built by value with [`with`](Self::with); a clone shares the loaders.
#[derive(Clone, Default)]
pub struct Catalog {
    loaders: IndexMap<String, Arc<dyn VistaResolver>>,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `loader` as model `name`, replacing any loader already
    /// registered under it. The loader is called with `name` each time the
    /// model is resolved.
    pub fn with(mut self, name: impl Into<String>, loader: impl VistaResolver + 'static) -> Self {
        self.loaders.insert(name.into(), Arc::new(loader));
        self
    }
}

impl VistaResolver for Catalog {
    /// A fresh Vista for model `name`. A name with no loader is not-found;
    /// a loader's own error is returned as it is, keeping its kind.
    fn vista(&self, name: &str) -> Result<Vista> {
        let loader = self
            .loaders
            .get(name)
            .ok_or_else(|| error!("model not in catalog", model = name).mark_not_found())?;
        loader.vista(name)
    }
}

impl fmt::Debug for Catalog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Catalog")
            .field("models", &self.loaders.keys().collect::<Vec<_>>())
            .finish()
    }
}
