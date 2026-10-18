//! `PostgresVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. PostgreSQL advertises full read/write/count.

use std::sync::Arc;

use vantage_core::Result;
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::{BuildEnv, Table, TableInjector};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Capability, ColumnSpec, NoExtras, SpecResolver, Vista, VistaFactory, VistaMetadata,
};

use crate::postgres::PostgresDB;
use crate::postgres::statements::PostgresSelect;
use crate::postgres::types::AnyPostgresType;
use crate::postgres::vista::source::PostgresTableShell;
use crate::postgres::vista::spec::{PostgresColumnExtras, PostgresTableExtras, PostgresVistaSpec};
use crate::sql_vista::{
    SourceBlock, SqlSpecDialect, build_spec_table, column_for_type, lower_spec, metadata_options,
    source_block, sql_capabilities,
};

pub struct PostgresVistaFactory {
    db: PostgresDB,
    resolver: Option<Arc<dyn SpecResolver<PostgresVistaSpec>>>,
    injector: Option<TableInjector<PostgresDB>>,
    notify: bool,
}

impl PostgresVistaFactory {
    pub fn new(db: PostgresDB) -> Self {
        Self {
            db,
            resolver: None,
            injector: None,
            notify: false,
        }
    }

    /// Attach a build injector, run on every table built from a spec: the
    /// root, each reference target when traversed, and each `base:` table.
    /// A table handed to [`from_table`](Self::from_table) is the caller's
    /// and is wrapped as it is.
    pub fn with_injector(mut self, injector: TableInjector<PostgresDB>) -> Self {
        self.injector = Some(injector);
        self
    }

    fn env(&self, key: &str) -> BuildEnv<PostgresVistaSpec, PostgresDB> {
        BuildEnv::root(key, self.resolver.clone(), self.injector.clone())
    }

    /// Attach a spec resolver, which finds a YAML spec by table name so
    /// references and `base:` can rebuild their tables from the live spec.
    /// Without it, a traversal yields a column-less target `Table` and the
    /// next query fails loudly.
    pub fn with_resolver(mut self, resolver: Arc<dyn SpecResolver<PostgresVistaSpec>>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Declare that this database has `{table}_changed` NOTIFY triggers
    /// installed, letting the vistas this factory builds advertise
    /// [`Subscribe`](Capability::Subscribe).
    ///
    /// Off by default, and deliberately explicit: Postgres cannot tell us
    /// whether a trigger exists. `LISTEN` on a channel nobody ever notifies
    /// succeeds and then blocks forever, so a vista that advertised
    /// `Subscribe` on the strength of being writable would promise a feed
    /// that is silent — indistinguishable, to a consumer, from a table where
    /// nothing happens. The trigger is application-installed (see `learn-10`'s
    /// `db::setup`, and the "Real-Time Push with LISTEN/NOTIFY" chapter), so
    /// the application is the only party that can honestly answer this.
    ///
    /// Applies to every table the factory builds; if only some of them carry
    /// triggers, use separate factories.
    pub fn with_notify(mut self, notify: bool) -> Self {
        self.notify = notify;
        self
    }

    pub fn from_table<E>(&self, table: Table<PostgresDB, E>) -> Result<Vista>
    where
        E: Entity<AnyPostgresType> + 'static,
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
        table: Table<PostgresDB, E>,
        name: String,
        metadata: VistaMetadata,
        read_only: bool,
    ) -> Vista
    where
        E: Entity<AnyPostgresType> + 'static,
    {
        let mut capabilities = sql_capabilities(read_only);
        // Push via LISTEN/NOTIFY on the `{table}_changed` channel, which the
        // application feeds from a trigger. Opt-in (`with_notify`): we cannot
        // detect the trigger, and claiming a feed we may never receive is
        // worse than claiming none.
        if self.notify && !read_only {
            capabilities = capabilities.with(Capability::Subscribe);
        }
        Vista::new(
            name,
            PostgresTableShell::new(table, capabilities, metadata, ()),
        )
    }

    /// Build a `Table<PostgresDB, EmptyEntity>` from a spec, resolving any
    /// `references:` against the attached resolver.
    pub fn table_from_spec(
        &self,
        spec: &PostgresVistaSpec,
    ) -> Result<Table<PostgresDB, EmptyEntity>> {
        build_spec_table(spec, self.db.clone(), &self.env(&spec.name))
    }
}

impl VistaFactory for PostgresVistaFactory {
    type TableExtras = PostgresTableExtras;
    type ColumnExtras = PostgresColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: PostgresVistaSpec) -> Result<Vista> {
        let lowered = lower_spec(&spec, self.db.clone(), &self.env(&spec.name))?;
        Ok(self.wrap(
            lowered.table,
            spec.name,
            lowered.metadata,
            lowered.read_only,
        ))
    }
}

impl SqlSpecDialect for PostgresDB {
    type TableExtras = PostgresTableExtras;
    type ColumnExtras = PostgresColumnExtras;
    type ReferenceExtras = NoExtras;
    type Query = PostgresSelect;

    fn source_block(spec: &PostgresVistaSpec) -> SourceBlock<'_> {
        match &spec.driver.postgres {
            Some(block) => source_block!(block),
            None => SourceBlock::default(),
        }
    }

    /// The `postgres.column` block names the physical column.
    fn spec_column(
        name: &str,
        spec: &ColumnSpec<PostgresColumnExtras>,
    ) -> Result<TableColumn<AnyPostgresType>> {
        let physical = spec.driver.postgres.as_ref().and_then(|b| b.column.clone());
        TableColumn::from_spec(name, spec, physical, column_for_type)
    }

    #[cfg(feature = "rhai")]
    fn eval_select(
        code: &str,
        base: Option<PostgresSelect>,
        args: &[(String, String)],
    ) -> Result<PostgresSelect> {
        crate::postgres::vista::rhai_source::eval_to_select_args(code, base, args)
    }
}
