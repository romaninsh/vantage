//! `SqliteVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. SQLite advertises full read/write/count.

use std::sync::Arc;

use vantage_core::Result;
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{ColumnSpec, NoExtras, SpecResolver, Vista, VistaFactory, VistaMetadata};

use crate::sql_vista::{
    SourceBlock, SqlSpecDialect, build_spec_table, column_for_type, lower_spec, metadata_options,
    source_block, sql_capabilities,
};
use crate::sqlite::SqliteDB;
use crate::sqlite::statements::SqliteSelect;
use crate::sqlite::types::AnySqliteType;
use crate::sqlite::vista::source::SqliteTableShell;
use crate::sqlite::vista::spec::{SqliteColumnExtras, SqliteTableExtras, SqliteVistaSpec};

pub struct SqliteVistaFactory {
    db: SqliteDB,
    resolver: Option<Arc<dyn SpecResolver<SqliteVistaSpec>>>,
}

impl SqliteVistaFactory {
    pub fn new(db: SqliteDB) -> Self {
        Self { db, resolver: None }
    }

    /// Attach a spec resolver, which finds a YAML spec by table name so
    /// references and `base:` can rebuild their tables from the live spec.
    /// Without it, a traversal yields a column-less target `Table` and the
    /// next query fails loudly.
    pub fn with_resolver(mut self, resolver: Arc<dyn SpecResolver<SqliteVistaSpec>>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Wrap a typed table as a Vista. Column metadata is harvested from the
    /// table; CRUD goes through `Table`'s reading path. The original entity
    /// type is preserved so `with_expression` closures remain typecheckable.
    pub fn from_table<E>(&self, table: Table<SqliteDB, E>) -> Result<Vista>
    where
        E: Entity<AnySqliteType> + 'static,
    {
        let name = table.table_name().to_string();
        let metadata = table.vista_metadata(metadata_options());
        Ok(self.wrap(table, name, metadata, false))
    }

    /// Single source-construction site shared by `from_table` and
    /// `build_from_spec`. A query-sourced (e.g. `rhai:`) vista is `read_only`,
    /// which clears the write capabilities.
    fn wrap<E>(
        &self,
        table: Table<SqliteDB, E>,
        name: String,
        metadata: VistaMetadata,
        read_only: bool,
    ) -> Vista
    where
        E: Entity<AnySqliteType> + 'static,
    {
        let shell = SqliteTableShell::new(table, sql_capabilities(read_only), metadata, ());
        Vista::new(name, shell)
    }

    /// Build a `Table<SqliteDB, EmptyEntity>` from a spec, resolving any
    /// `references:` against the attached resolver.
    pub fn table_from_spec(&self, spec: &SqliteVistaSpec) -> Result<Table<SqliteDB, EmptyEntity>> {
        build_spec_table(spec, self.db.clone(), self.resolver.clone())
    }
}

impl VistaFactory for SqliteVistaFactory {
    type TableExtras = SqliteTableExtras;
    type ColumnExtras = SqliteColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: SqliteVistaSpec) -> Result<Vista> {
        let lowered = lower_spec(&spec, self.db.clone(), self.resolver.clone())?;
        Ok(self.wrap(
            lowered.table,
            spec.name,
            lowered.metadata,
            lowered.read_only,
        ))
    }
}

impl SqlSpecDialect for SqliteDB {
    type TableExtras = SqliteTableExtras;
    type ColumnExtras = SqliteColumnExtras;
    type ReferenceExtras = NoExtras;
    type Query = SqliteSelect;

    fn source_block(spec: &SqliteVistaSpec) -> SourceBlock<'_> {
        match &spec.driver.sqlite {
            Some(block) => source_block!(block),
            None => SourceBlock::default(),
        }
    }

    /// The `sqlite.column` block names the physical column.
    fn spec_column(
        name: &str,
        spec: &ColumnSpec<SqliteColumnExtras>,
    ) -> Result<TableColumn<AnySqliteType>> {
        let physical = spec.driver.sqlite.as_ref().and_then(|b| b.column.clone());
        TableColumn::from_spec(name, spec, physical, column_for_type)
    }

    #[cfg(feature = "rhai")]
    fn eval_select(
        code: &str,
        base: Option<SqliteSelect>,
        args: &[(String, String)],
    ) -> Result<SqliteSelect> {
        crate::sqlite::vista::rhai_source::eval_to_select_args(code, base, args)
    }
}
