//! Assembling a MySQL Vista around a built table: its capabilities and,
//! with `rhai`, the `modify:` script.

use vantage_core::Result;
#[cfg(not(feature = "rhai"))]
use vantage_core::error;
use vantage_table::table::Table;
use vantage_types::Entity;
use vantage_vista::{Capability, Vista, VistaFactory, VistaMetadata};

use super::MysqlVistaFactory;
use crate::mysql::MysqlDB;
use crate::mysql::types::AnyMysqlType;
use crate::mysql::vista::source::MysqlTableShell;
use crate::mysql::vista::spec::{
    MysqlColumnExtras, MysqlReferenceExtras, MysqlTableExtras, MysqlVistaSpec,
};
use crate::sql_vista::{lower_spec, sql_capabilities};

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
        let modify = spec.driver.mysql.as_ref().and_then(|m| m.modify.clone());
        #[cfg(not(feature = "rhai"))]
        if modify.is_some() {
            return Err(error!(
                "vista declares a `modify:` script but vantage-sql was built without the `rhai` feature",
                vista = spec.name.as_str()
            ));
        }

        let lowered = lower_spec(&spec, self.db.clone(), self.resolver.clone())?;
        #[cfg_attr(not(feature = "rhai"), allow(unused_mut))]
        let mut vista = self.wrap(
            lowered.table,
            spec.name,
            lowered.metadata,
            lowered.read_only,
        );

        // Final step: a `mysql: { modify }` script tweaks the built vista, so it
        // composes with `table`/`rhai`/`base`.
        #[cfg(feature = "rhai")]
        if let Some(code) = modify {
            vista = self.apply_modify(vista, &code)?;
        }
        Ok(vista)
    }
}
