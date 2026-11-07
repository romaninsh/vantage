//! A built table's Vista metadata, and a spec's references lowered into it.

use vantage_core::{Result, error};
use vantage_table::table::spec::spec_references;
use vantage_table::table::{Orderable, Table, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{Reference, ReferenceKind, ReferenceSpec, VistaMetadata};

use crate::surrealdb::SurrealDB;
use crate::types::AnySurrealType;
use crate::vista::spec::{SurrealReferenceExtras, SurrealVistaSpec};

pub(super) fn metadata_from_table<E>(table: &Table<SurrealDB, E>) -> VistaMetadata
where
    E: Entity<AnySurrealType> + 'static,
{
    // Every column is orderable. An imported implicit-reference column
    // (`customer.name`) orders by its projected alias, which the order
    // renders escaped (`ORDER BY ⟨customer.name⟩`): bare, it would address a
    // field path and silently not sort. expr:/lazy computed columns order by
    // their alias the same way; all computed columns are read-only for
    // consumers.
    table.vista_metadata(VistaMetadataOptions {
        orderable: Orderable::All,
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
    let derived = spec
        .driver
        .surreal
        .as_ref()
        .is_some_and(|b| b.base.is_some());
    let mut metadata = metadata_from_table(table);
    for mut reference in spec_references(spec, &table.id_field_name())? {
        if !metadata.references.contains_key(&reference.name) {
            return Err(error!(
                "spec reference is absent from the built table's references",
                relation = reference.name.as_str(),
                table = table.table_name()
            ));
        }
        // A derived table's rows carry only its columns, so a has_one keyed
        // by any other field has no value to join on.
        if derived && reference.kind == ReferenceKind::HasOne {
            for key in &reference.keys {
                if !table.columns().contains_key(&key.from) {
                    return Err(error!(
                        "a derived table's has_one reference needs its foreign key as a column",
                        relation = reference.name.as_str(),
                        column = key.from.as_str(),
                        table = table.table_name()
                    ));
                }
            }
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
