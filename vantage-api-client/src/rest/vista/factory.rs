//! `RestApiVistaFactory` — typed-table entry point and `VistaFactory`
//! trait impl. REST API is read-only at this stage, so the factory
//! advertises only `can_count`.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::Entity;
use vantage_vista::{
    Column as VistaColumn, Reference as VistaReference, ReferenceKind, Vista, VistaCapabilities,
    VistaFactory, VistaMetadata, flags as vista_flags,
};

use crate::RestApi;
use super::source::RestApiTableShell;
use super::spec::{NoApiExtras, RestApiVistaSpec};

pub struct RestApiVistaFactory {
    api: RestApi,
}

impl RestApiVistaFactory {
    pub fn new(api: RestApi) -> Self {
        Self { api }
    }

    pub fn api(&self) -> &RestApi {
        &self.api
    }

    /// Wrap a typed `Table<RestApi, E>` as a `Vista`. Column metadata,
    /// id field, title fields, and references are harvested up front;
    /// the table itself is stored as `Box<dyn TableLike>` so the
    /// original `E` stays attached for reference traversal.
    pub fn from_table<E>(&self, table: Table<RestApi, E>) -> Result<Vista>
    where
        E: Entity<CborValue> + 'static,
    {
        let metadata = metadata_from_table(&table);
        let name = table.table_name().to_string();

        let source = RestApiTableShell::new(
            Box::new(table),
            VistaCapabilities {
                can_count: true,
                ..VistaCapabilities::default()
            },
        );
        Ok(Vista::new(name, Box::new(source), metadata))
    }
}

impl VistaFactory for RestApiVistaFactory {
    type TableExtras = NoApiExtras;
    type ColumnExtras = NoApiExtras;
    type ReferenceExtras = NoApiExtras;

    fn build_from_spec(&self, _spec: RestApiVistaSpec) -> Result<Vista> {
        Err(vantage_core::error!(
            "YAML spec construction not yet implemented for RestApiVistaFactory"
        ))
    }
}

fn metadata_from_table<E>(table: &Table<RestApi, E>) -> VistaMetadata
where
    E: Entity<CborValue> + 'static,
{
    let mut metadata = VistaMetadata::new();
    for (name, col) in table.columns() {
        let mut vc = VistaColumn::new(name.clone(), col.get_type().to_string());
        if col.flags().contains(&ColumnFlag::Hidden) {
            vc = vc.with_flag(vista_flags::HIDDEN);
        }
        metadata = metadata.with_column(vc);
    }
    if let Some(id_field) = table.id_field() {
        let id = id_field.name().to_string();
        metadata = metadata.with_id_column(id.clone());
        if let Some(col) = metadata.columns.get_mut(&id) {
            col.flags.push(vista_flags::ID.to_string());
        }
    }
    for title in table.title_fields() {
        if let Some(col) = metadata.columns.get_mut(title) {
            col.flags.push(vista_flags::TITLE.to_string());
        }
    }
    for relation in table.references() {
        metadata = metadata.with_reference(VistaReference::new(
            relation.clone(),
            // Target / foreign-key metadata are driver-internal for
            // REST API (the typed table's reference closure owns the
            // child build); surface a placeholder so the universal
            // shape stays populated.
            "",
            ReferenceKind::HasMany,
            "",
        ));
    }
    metadata
}
