//! A built table's Vista metadata, and a spec's references lowered into it.

use vantage_core::{Result, error};
use vantage_table::table::spec::spec_references;
use vantage_table::table::{Orderable, Table, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{Reference, ReferenceSpec, VistaMetadata};

use crate::surrealdb::SurrealDB;
use crate::types::AnySurrealType;
use crate::vista::spec::{SurrealReferenceExtras, SurrealVistaSpec};

pub(super) fn metadata_from_table<E>(table: &Table<SurrealDB, E>) -> VistaMetadata
where
    E: Entity<AnySurrealType> + 'static,
{
    // SurrealDB sorts on any physical field. An imported implicit-reference
    // column exists only as a projection alias — ordering by its dotted name
    // would address a nonexistent field and silently not sort. expr:/lazy
    // computed columns stay orderable (they order by their projected alias);
    // all computed columns are read-only for consumers.
    table.vista_metadata(VistaMetadataOptions {
        orderable: Orderable::ExceptImported,
        calculated: true,
        column_flags: true,
        references: true,
        contained: true,
        ..VistaMetadataOptions::default()
    })
}

/// Metadata for a table built from `spec`. Each reference, a `references:`
/// entry or a column-level `references:`, is lowered from the spec, so its
/// target is the spec name a resolver knows (which may differ from the
/// SurrealDB table the target reads) and its `surreal.rhai` script rides
/// along as the build script. Relations a derived table inherits keep the
/// join its base built.
pub(super) fn metadata_from_spec(
    table: &Table<SurrealDB, EmptyEntity>,
    spec: &SurrealVistaSpec,
) -> Result<VistaMetadata> {
    let mut metadata = metadata_from_table(table);
    for mut reference in spec_references(spec, &table.id_field_name())? {
        // A `base:` vista builds no references of its own, only the
        // relations it inherits, so a traversal would find no target.
        if !metadata.references.contains_key(&reference.name) {
            return Err(error!(
                "spec reference is absent from the built table's references",
                relation = reference.name.as_str(),
                table = table.table_name()
            ));
        }
        if let Some(code) = spec.references.get(&reference.name).and_then(build_script) {
            reference = with_build_script(reference, code, &spec.name)?;
        }
        metadata = metadata.with_reference(reference);
    }
    Ok(metadata)
}

fn build_script(reference: &ReferenceSpec<SurrealReferenceExtras>) -> Option<String> {
    reference.driver.surreal.as_ref()?.rhai.clone()
}

#[cfg(feature = "rhai")]
fn with_build_script(reference: Reference, code: String, _vista: &str) -> Result<Reference> {
    reference.with_build_script(code)
}

#[cfg(not(feature = "rhai"))]
fn with_build_script(reference: Reference, _code: String, vista: &str) -> Result<Reference> {
    Err(error!(
        "reference declares a `rhai:` build script but vantage-surrealdb was built without the `rhai` feature",
        vista = vista,
        relation = reference.name.as_str()
    ))
}
