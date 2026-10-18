//! `MysqlVistaFactory` — typed-table and YAML entry points. MySQL advertises
//! read, write, order, search, windowed reads, native conditions,
//! transactional import and traversal (scripted references with `rhai`); a
//! query-sourced vista is read-only.
//!
//! This file holds the factory and what spec lowering asks of MySQL;
//! `assemble` wraps a table as a Vista and implements `VistaFactory`, and
//! `scripting` hosts the Rhai scripts.

mod assemble;
#[cfg(feature = "rhai")]
mod scripting;

use std::sync::Arc;

use vantage_core::Result;
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::{BuildEnv, Table, TableInjector};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{ColumnSpec, ReferenceSpec, SpecResolver, Vista};

use crate::mysql::MysqlDB;
use crate::mysql::statements::MysqlSelect;
use crate::mysql::types::AnyMysqlType;
use crate::mysql::vista::spec::{
    MysqlColumnExtras, MysqlReferenceExtras, MysqlTableExtras, MysqlVistaSpec,
};
use crate::sql_vista::{
    SourceBlock, SqlSpecDialect, build_spec_table, column_for_type, metadata_options, source_block,
};

#[derive(Clone)]
pub struct MysqlVistaFactory {
    db: MysqlDB,
    resolver: Option<Arc<dyn SpecResolver<MysqlVistaSpec>>>,
    injector: Option<TableInjector<MysqlDB>>,
}

impl MysqlVistaFactory {
    pub fn new(db: MysqlDB) -> Self {
        Self {
            db,
            resolver: None,
            injector: None,
        }
    }

    /// Attach a build injector, run on every table built from a spec: the
    /// root, each reference target when traversed, each `base:` table, and
    /// the tables a script's `table(name)` builds. A table handed to
    /// [`from_table`](Self::from_table) is the caller's and is wrapped as it
    /// is.
    pub fn with_injector(mut self, injector: TableInjector<MysqlDB>) -> Self {
        self.injector = Some(injector);
        self
    }

    fn env(&self, key: &str) -> BuildEnv<MysqlVistaSpec, MysqlDB> {
        BuildEnv::root(key, self.resolver.clone(), self.injector.clone())
    }

    /// Attach a spec resolver, which finds a YAML spec by table name so
    /// references and `base:` can rebuild their tables from the live spec.
    /// Without it, a traversal yields a column-less target `Table` and the
    /// next query fails loudly.
    pub fn with_resolver(mut self, resolver: Arc<dyn SpecResolver<MysqlVistaSpec>>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    pub fn from_table<E>(&self, table: Table<MysqlDB, E>) -> Result<Vista>
    where
        E: Entity<AnyMysqlType> + 'static,
    {
        let name = table.table_name().to_string();
        let metadata = table.vista_metadata(metadata_options());
        Ok(self.wrap(table, name, metadata, false))
    }

    /// Build a `Table<MysqlDB, EmptyEntity>` from a spec, resolving any
    /// `references:` against the attached resolver.
    pub fn table_from_spec(&self, spec: &MysqlVistaSpec) -> Result<Table<MysqlDB, EmptyEntity>> {
        build_spec_table(spec, self.db.clone(), &self.env(&spec.name))
    }
}

impl SqlSpecDialect for MysqlDB {
    type TableExtras = MysqlTableExtras;
    type ColumnExtras = MysqlColumnExtras;
    type ReferenceExtras = MysqlReferenceExtras;
    type Query = MysqlSelect;

    fn source_block(spec: &MysqlVistaSpec) -> SourceBlock<'_> {
        match &spec.driver.mysql {
            Some(block) => source_block!(block),
            None => SourceBlock::default(),
        }
    }

    /// The `mysql.column` block names the physical column.
    fn spec_column(
        name: &str,
        spec: &ColumnSpec<MysqlColumnExtras>,
    ) -> Result<TableColumn<AnyMysqlType>> {
        let physical = spec.driver.mysql.as_ref().and_then(|b| b.column.clone());
        TableColumn::from_spec(name, spec, physical, column_for_type)
    }

    fn reference_script(reference: &ReferenceSpec<MysqlReferenceExtras>) -> Option<String> {
        reference.driver.mysql.as_ref()?.rhai.clone()
    }

    #[cfg(feature = "rhai")]
    fn eval_select(
        code: &str,
        base: Option<MysqlSelect>,
        args: &[(String, String)],
    ) -> Result<MysqlSelect> {
        crate::mysql::vista::rhai_source::eval_to_select_args(code, base, args)
    }
}
