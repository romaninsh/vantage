//! Assembling a MySQL Vista around a built table: its capabilities, the
//! reference build-scripts and, with `rhai`, the `modify:` script.

use vantage_core::Result;
#[cfg(not(feature = "rhai"))]
use vantage_core::error;
use vantage_table::table::Table;
use vantage_types::Entity;
use vantage_vista::{Capability, ReferenceSpec, Vista, VistaFactory, VistaMetadata};

use super::MysqlVistaFactory;
use crate::mysql::MysqlDB;
use crate::mysql::types::AnyMysqlType;
use crate::mysql::vista::source::MysqlTableShell;
use crate::mysql::vista::spec::{
    MysqlColumnExtras, MysqlReferenceExtras, MysqlTableExtras, MysqlVistaSpec,
};
use crate::sql_vista::{metadata_options, sql_capabilities};

impl MysqlVistaFactory {
    /// Single source-construction site shared by `from_table` and
    /// `build_from_spec`. A query-sourced (e.g. `rhai:`) vista is `read_only`,
    /// which clears the write capabilities.
    pub(super) fn wrap<E>(
        &self,
        table: Table<MysqlDB, E>,
        name: String,
        metadata: VistaMetadata,
        read_only: bool,
    ) -> Vista
    where
        E: Entity<AnyMysqlType> + 'static,
    {
        // Native conditions narrow a read-only vista too; import is a write.
        let mut capabilities = sql_capabilities(read_only).with(Capability::NativeConditions);
        if !read_only {
            capabilities = capabilities.with(Capability::Import);
        }
        let shell = MysqlTableShell::new(table, capabilities, metadata, self.resolver.clone());
        Vista::new(name, shell)
    }
}

impl VistaFactory for MysqlVistaFactory {
    type TableExtras = MysqlTableExtras;
    type ColumnExtras = MysqlColumnExtras;
    type ReferenceExtras = MysqlReferenceExtras;

    fn build_from_spec(&self, spec: MysqlVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let read_only = spec
            .driver
            .mysql
            .as_ref()
            .is_some_and(|m| m.rhai.is_some() || m.base.is_some());
        let script =
            |r: &ReferenceSpec<MysqlReferenceExtras>| r.driver.mysql.as_ref()?.rhai.clone();
        let modify = spec.driver.mysql.as_ref().and_then(|m| m.modify.clone());
        #[cfg(not(feature = "rhai"))]
        if let Some((relation, _)) = spec.references.iter().find(|(_, r)| script(r).is_some()) {
            return Err(error!(
                "reference declares a `rhai:` build script but vantage-sql was built without the `rhai` feature",
                vista = vista_name.as_str(),
                relation = relation.as_str()
            ));
        }
        #[cfg(not(feature = "rhai"))]
        if modify.is_some() {
            return Err(error!(
                "vista declares a `modify:` script but vantage-sql was built without the `rhai` feature",
                vista = vista_name.as_str()
            ));
        }

        let table = self.table_from_spec(&spec)?;
        let mut metadata = table.vista_metadata(metadata_options());
        table.name_spec_targets(&mut metadata, &spec.references, script)?;
        #[cfg_attr(not(feature = "rhai"), allow(unused_mut))]
        let mut vista = self.wrap(table, vista_name, metadata, read_only);

        // Final step: a `mysql: { modify }` script tweaks the built vista, so it
        // composes with `table`/`rhai`/`base`.
        #[cfg(feature = "rhai")]
        if let Some(code) = modify {
            vista = self.apply_modify(vista, &code)?;
        }
        Ok(vista)
    }
}
