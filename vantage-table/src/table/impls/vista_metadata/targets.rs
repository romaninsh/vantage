//! Naming a YAML spec's reference targets in Vista metadata.
//!
//! [`Table::vista_references`] names each join target by its built table's
//! name. A YAML reference names its target by spec name instead, which may
//! differ from the table or collection the spec reads, so a driver factory
//! overlays the spec's names.

use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Entity;
use vantage_vista::{ReferenceSpec, VistaMetadata};

use crate::{table::Table, traits::table_source::TableSource};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Overlay a spec's `references:` onto `metadata`, this table's
    /// metadata: each target is the spec name the reference declares (a
    /// resolver's key), and `script` gives its build script, if any.
    /// Relations the spec doesn't declare (inherited by a derived table)
    /// keep their built table's name. A spec reference the table lacks is an
    /// error.
    pub fn name_spec_targets<R>(
        &self,
        metadata: &mut VistaMetadata,
        references: &IndexMap<String, ReferenceSpec<R>>,
        script: impl Fn(&ReferenceSpec<R>) -> Option<String>,
    ) -> Result<()> {
        for (relation, spec) in references {
            let reference = metadata.references.get_mut(relation).ok_or_else(|| {
                error!(
                    "spec reference is absent from the built table's references",
                    relation = relation.as_str(),
                    table = self.table_name()
                )
            })?;
            reference.target = spec.table.clone();
            if let Some(code) = script(spec) {
                *reference = reference.clone().with_build_script(code)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "targets_tests.rs"]
mod tests;
