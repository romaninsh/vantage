//! Lowering: [`AugmentSpec`] (serde) → [`Augmentation`] (runtime closures). The
//! only place Rhai is touched — and only behind the `rhai` feature.

use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_vista_factory::VistaCatalog;

use super::spec::{AugmentSpec, FetchSpec, SourceSpec};
use super::{Augmentation, Fetch, MergeRule, Source};

/// Lower one declared augmentation into its runtime form. `catalog` backs the
/// `table(name)` resolver used by scripted sources.
pub fn lower_augment(spec: AugmentSpec, catalog: &Arc<VistaCatalog>) -> Result<Augmentation> {
    let source = match spec.source {
        SourceSpec::Id => Source::Id,
        SourceSpec::Column { from, to } => Source::Column { from, to },
        SourceSpec::Script { code } => lower_source_script(code, catalog)?,
    };
    let fetch = match spec.fetch {
        FetchSpec::PerRow => Fetch::PerRow,
        FetchSpec::Batched { op } => Fetch::Batched { op },
        FetchSpec::Script { .. } => {
            return Err(error!(
                "augment: scripted fetch is not yet implemented (phase 2)"
            ));
        }
    };
    Ok(Augmentation {
        table: spec.table,
        source,
        fetch,
        merge: MergeRule {
            columns: spec.merge,
        },
    })
}

#[cfg(feature = "rhai")]
fn lower_source_script(code: String, catalog: &Arc<VistaCatalog>) -> Result<Source> {
    let catalog = catalog.clone();
    let resolver: vantage_vista::TargetResolver =
        Arc::new(move |name: &str| catalog.build_vista(name));
    Ok(Source::Build(vantage_vista::augment_source_closure(
        resolver, code,
    )))
}

#[cfg(not(feature = "rhai"))]
fn lower_source_script(_code: String, _catalog: &Arc<VistaCatalog>) -> Result<Source> {
    Err(error!(
        "augment: source `script` requires the `rhai` feature"
    ))
}
