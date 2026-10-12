//! A built table wrapped as a `Vista`, and the `VistaFactory` entry point
//! that builds one from a spec.

use indexmap::IndexMap;
use vantage_core::Result;
#[cfg(not(feature = "rhai"))]
use vantage_core::error;
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

/// The table's Vista metadata. Join targets carry their built table's name;
/// a spec entry point renames them to spec names (`name_spec_targets`).
pub(super) fn mongo_metadata(table: &Table<MongoDB, EmptyEntity>) -> VistaMetadata {
    // MongoDB sorts on any field.
    table.vista_metadata(VistaMetadataOptions {
        orderable: Orderable::All,
        references: true,
        contained: true,
        column_flags: true,
        ..VistaMetadataOptions::default()
    })
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
        let script =
            |r: &ReferenceSpec<MongoReferenceExtras>| r.driver.mongo.as_ref()?.rhai.clone();
        #[cfg(not(feature = "rhai"))]
        if let Some((relation, _)) = spec.references.iter().find(|(_, r)| script(r).is_some()) {
            return Err(error!(
                "reference declares a `rhai:` build script but vantage-mongodb was built without the `rhai` feature",
                vista = spec.name.as_str(),
                relation = relation.as_str()
            ));
        }
        let table = self.table_from_spec(&spec)?;
        let mut metadata = mongo_metadata(&table);
        table.name_spec_targets(&mut metadata, &spec.references, script)?;
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
