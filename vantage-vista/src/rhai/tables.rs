//! The `table(name)` resolver a driver's script host hands to [`DataVocab`]:
//! look the name up as a spec, then build it through the driver's factory.
//!
//! [`DataVocab`]: super::DataVocab

use std::sync::Arc;

use vantage_core::{Result, error};

use crate::{SpecResolver, VistaFactory, VistaResolver, VistaSpec};

type FactorySpec<F> = VistaSpec<
    <F as VistaFactory>::TableExtras,
    <F as VistaFactory>::ColumnExtras,
    <F as VistaFactory>::ReferenceExtras,
>;

/// Build a script's `table(name)` from the spec `specs` resolves, through
/// `factory`. A name `specs` doesn't know is not-found rather than a bare
/// table, so a typo fails loudly.
///
/// Without `specs`, `bare` builds the spec for every name; a driver whose
/// tables need declared columns passes `None`, and then every `table(name)`
/// is an error.
pub fn script_tables<F: VistaFactory>(
    factory: F,
    specs: Option<Arc<dyn SpecResolver<FactorySpec<F>>>>,
    bare: Option<fn(&str) -> FactorySpec<F>>,
) -> Arc<dyn VistaResolver> {
    Arc::new(move |name: &str| factory.build_from_spec(script_spec(specs.as_deref(), bare, name)?))
}

fn script_spec<S>(
    specs: Option<&dyn SpecResolver<S>>,
    bare: Option<fn(&str) -> S>,
    name: &str,
) -> Result<S> {
    match (specs, bare) {
        (Some(specs), _) => specs
            .resolve(name)
            .ok_or_else(|| error!("script table(): unknown table", table = name).mark_not_found()),
        (None, Some(bare)) => Ok(bare(name)),
        (None, None) => Err(error!(
            "script table() requires a spec resolver on the factory"
        )),
    }
}
