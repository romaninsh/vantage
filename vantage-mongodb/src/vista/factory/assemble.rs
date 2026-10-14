//! A built table wrapped as a `Vista`, and the `VistaFactory` entry point
//! that builds one from a spec.

use indexmap::IndexMap;
use vantage_core::Result;
#[cfg(not(feature = "rhai"))]
use vantage_core::error;
use vantage_table::table::spec::spec_references;
use vantage_table::table::{Orderable, Table, VistaMetadataOptions};
use vantage_types::EmptyEntity;
use vantage_vista::{ReferenceSpec, Vista, VistaFactory, VistaMetadata};

#[cfg(not(feature = "rhai"))]
use super::build::missing_rhai;
use super::{MongoVistaFactory, is_bare, mongo_capabilities, paths_from_table_columns};
use crate::mongodb::MongoDB;
use crate::vista::source::MongoTableShell;
use crate::vista::spec::{
    MongoColumnExtras, MongoReferenceExtras, MongoTableExtras, MongoVistaSpec,
};

/// MongoDB sorts on any field and carries every column flag.
fn metadata_options() -> VistaMetadataOptions {
    VistaMetadataOptions {
        orderable: Orderable::All,
        column_flags: true,
        ..VistaMetadataOptions::default()
    }
}

/// A typed table's Vista metadata. Join targets carry their built table's
/// name.
pub(super) fn mongo_metadata(table: &Table<MongoDB, EmptyEntity>) -> VistaMetadata {
    table.vista_metadata(VistaMetadataOptions {
        references: true,
        contained: true,
        ..metadata_options()
    })
}

/// A spec-built table's Vista metadata. Each reference, a `references:`
/// entry or a column-level `references:`, is lowered from the spec, so its
/// target is the spec name a resolver knows (not the collection the target
/// reads) and a `mongo: { rhai: }` script rides along as its build script.
/// Contained relations come from the table.
fn spec_metadata(
    table: &Table<MongoDB, EmptyEntity>,
    spec: &MongoVistaSpec,
) -> Result<VistaMetadata> {
    let mut metadata = table.vista_metadata(metadata_options());
    for mut lowered in spec_references(spec, &table.id_field_name())? {
        if let Some(code) = spec.references.get(&lowered.name).and_then(build_script) {
            lowered = lowered.with_build_script(code)?;
        }
        metadata = metadata.with_reference(lowered);
    }
    Ok(table
        .vista_contained()
        .into_iter()
        .fold(metadata, VistaMetadata::with_reference))
}

fn build_script(reference: &ReferenceSpec<MongoReferenceExtras>) -> Option<String> {
    reference.driver.mongo.as_ref()?.rhai.clone()
}

impl MongoVistaFactory {
    /// Wrap `table` and its `metadata` in a shell with the factory's
    /// capabilities, resolver and live-update choice. `column_paths` maps
    /// each column to its BSON path (table aliases or spec `nested_path`).
    pub(super) fn wrap_with_paths(
        &self,
        table: Table<MongoDB, EmptyEntity>,
        column_paths: IndexMap<String, Vec<String>>,
        name: String,
        search_unflagged_columns: bool,
        metadata: VistaMetadata,
    ) -> Vista {
        let writable = table.data_source().is_stored();
        let capabilities = mongo_capabilities(writable, writable && self.live_enabled());
        let shell = MongoTableShell::new(
            table,
            capabilities,
            metadata,
            column_paths,
            self.resolver.clone(),
            search_unflagged_columns,
            self.change_streams,
        );
        Vista::new(name, shell)
    }
}

impl VistaFactory for MongoVistaFactory {
    type TableExtras = MongoTableExtras;
    type ColumnExtras = MongoColumnExtras;
    type ReferenceExtras = MongoReferenceExtras;

    fn build_from_spec(&self, spec: MongoVistaSpec) -> Result<Vista> {
        let modify = spec.driver.mongo.as_ref().and_then(|b| b.modify.clone());
        #[cfg(not(feature = "rhai"))]
        if modify.is_some() {
            return Err(missing_rhai("modify"));
        }
        #[cfg(not(feature = "rhai"))]
        if let Some((relation, _)) = spec
            .references
            .iter()
            .find(|(_, r)| build_script(r).is_some())
        {
            return Err(error!(
                "reference declares a `rhai:` build script but vantage-mongodb was built without the `rhai` feature",
                vista = spec.name.as_str(),
                relation = relation.as_str()
            ));
        }
        let table = self.table_from_spec(&spec)?;
        let metadata = spec_metadata(&table, &spec)?;
        // A spec that declares nothing beyond `_id` reads documents as stored,
        // like a bare `from_table`, instead of projecting them down to `_id`.
        let column_paths = if is_bare(&table) {
            IndexMap::new()
        } else {
            paths_from_table_columns(&table)
        };
        let vista = self.wrap_with_paths(table, column_paths, spec.name, false, metadata);

        // Last step: a `modify:` script narrows or tweaks the built vista.
        #[cfg(feature = "rhai")]
        if let Some(code) = modify {
            return crate::vista::scripting::apply_modify(self, vista, &code);
        }
        Ok(vista)
    }
}
