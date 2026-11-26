//! `SqliteVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. SQLite advertises full read/write/count.

use std::sync::Arc;

use vantage_core::Result;
use vantage_expressions::{Expression, Expressive as _};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::{BuildEnv, Table, TableInjector};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    BuildContext, ColumnSpec, NoExtras, SpecResolver, Vista, VistaFactory, VistaMetadata,
};

use crate::primitives::identifier::ident;
use crate::primitives::json_extract::json_extract;
use crate::sql_vista::{
    SourceBlock, SqlSpecDialect, build_spec_table, column_for_type, is_query_sourced, lower_spec,
    metadata_options, source_block, sql_capabilities,
};
use crate::sqlite::SqliteDB;
use crate::sqlite::statements::SqliteSelect;
use crate::sqlite::types::AnySqliteType;
use crate::sqlite::vista::source::SqliteTableShell;
use crate::sqlite::vista::spec::{SqliteColumnExtras, SqliteTableExtras, SqliteVistaSpec};

pub struct SqliteVistaFactory {
    db: SqliteDB,
    resolver: Option<Arc<dyn SpecResolver<SqliteVistaSpec>>>,
    injector: Option<TableInjector<SqliteDB>>,
}

impl SqliteVistaFactory {
    pub fn new(db: SqliteDB) -> Self {
        Self {
            db,
            resolver: None,
            injector: None,
        }
    }

    /// Attach a build injector, run on every table built from a spec: the
    /// root, each reference target when traversed, and each `base:` table.
    /// A table handed to [`from_table`](Self::from_table) is the caller's
    /// and is wrapped as it is.
    pub fn with_injector(mut self, injector: TableInjector<SqliteDB>) -> Self {
        self.injector = Some(injector);
        self
    }

    fn env(&self, key: &str, cx: &BuildContext) -> BuildEnv<SqliteVistaSpec, SqliteDB> {
        BuildEnv::root(key, self.resolver.clone(), self.injector.clone()).with_context(cx.clone())
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
        let read_only = is_query_sourced(table.source());
        Ok(self.wrap(table, name, metadata, read_only))
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
        let shell = SqliteTableShell::new(
            table,
            sql_capabilities(read_only).with(vantage_vista::Capability::NativeConditions),
            metadata,
            (),
        );
        Vista::new(name, shell)
    }

    /// Build a `Table<SqliteDB, EmptyEntity>` from a spec, resolving any
    /// `references:` against the attached resolver.
    pub fn table_from_spec(&self, spec: &SqliteVistaSpec) -> Result<Table<SqliteDB, EmptyEntity>> {
        let env = self.env(&spec.name, &BuildContext::default());
        build_spec_table(spec, self.db.clone(), &env)
    }
}

impl VistaFactory for SqliteVistaFactory {
    type TableExtras = SqliteTableExtras;
    type ColumnExtras = SqliteColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: SqliteVistaSpec) -> Result<Vista> {
        self.build_from_spec_in(spec, &BuildContext::default())
    }

    fn build_from_spec_in(&self, spec: SqliteVistaSpec, cx: &BuildContext) -> Result<Vista> {
        let lowered = lower_spec(&spec, self.db.clone(), &self.env(&spec.name, cx))?;
        // Only here, on the table the caller asked for: a reference target
        // or a `base:` table checking its own would build this one again.
        lowered.table.check_expressions()?;
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
        let physical = spec
            .driver
            .sqlite
            .as_ref()
            .and_then(|b| b.column.clone())
            .or_else(|| spec.nested_path.clone());
        TableColumn::from_spec(name, spec, physical, column_for_type)
    }

    fn nested_read(host: &str, key: &str) -> Expression<AnySqliteType> {
        json_extract(ident(host), vec![key.to_string()]).expr()
    }

    #[cfg(feature = "rhai")]
    fn eval_column_expr(
        code: &str,
        table: Option<Table<Self, vantage_types::EmptyEntity>>,
        env: vantage_rhai::Env,
    ) -> Result<
        vantage_expressions::Expression<
            <Self as vantage_table::traits::table_source::TableSource>::Value,
        >,
    > {
        crate::sqlite::vista::rhai_source::eval_column_expr(code, table, env)
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
