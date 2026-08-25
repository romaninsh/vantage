//! `PostgresTableShell` — owns the typed `Table<PostgresDB, E>` and exposes
//! it through the `TableShell` boundary. The shell is generic in `E` so that
//! `with_expression` closures (parameterized over `E`) survive the wrap;
//! `Vista` erases `E` once at the `Box<dyn TableShell>` boundary.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::conditions::ConditionHandle;
use vantage_table::pagination::Pagination;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{
    Column as VistaColumn, ContainedSpec, Reference as VistaReference, SortDirection, TableShell,
    Vista, VistaCapabilities, VistaChange, VistaChangeStream, VistaMetadata,
};

use crate::postgres::PostgresDB;
use crate::postgres::operation::PostgresOperation;
use crate::postgres::types::AnyPostgresType;
use crate::primitives::identifier::ident;
use crate::types::{cbor_to_json, parse_json_host};

#[derive(Clone)]
pub struct PostgresTableShell<E = EmptyEntity>
where
    E: Entity<AnyPostgresType>,
{
    pub(crate) table: Table<PostgresDB, E>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// Handle for the active quicksearch condition (if any). Used by
    /// `clear_search` and by `add_search`'s replace-semantics to remove the
    /// previous search before pushing the new one.
    pub(crate) current_search_handle: Option<ConditionHandle>,
    /// Pages-per-fetch declared via `set_page_size`. `None` until the consumer
    /// declares it; `fetch_page` errors with a clear message in that case.
    pub(crate) page_size: Option<usize>,
}

impl<E> PostgresTableShell<E>
where
    E: Entity<AnyPostgresType>,
{
    pub(crate) fn new(
        table: Table<PostgresDB, E>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
            current_search_handle: None,
            page_size: None,
        }
    }

    /// Whether the application declared `{table}_changed` triggers for this
    /// database (see `PostgresVistaFactory::with_notify`).
    ///
    /// Traversals build a fresh factory, which would otherwise reset the opt-in
    /// and leave every relation target silently unwatchable on a database that
    /// does have triggers. We recover it from our own advertised capability
    /// rather than storing it twice. A query-sourced parent reads back `false`
    /// (it is read-only, so it never advertised), which under-advertises a real
    /// child table — the safe direction: a missed feed degrades to the
    /// consumer's reconcile path, a phantom one degrades to silence.
    fn notify_opt_in(&self) -> bool {
        self.capabilities.can_subscribe
    }

    /// Give a paged read a deterministic order.
    ///
    /// `LIMIT`/`OFFSET` without `ORDER BY` is not a stable window: Postgres
    /// is free to return rows in any order, and a parallel or bitmap scan
    /// readily does — so successive pages can repeat a row and skip
    /// another, and the caller never learns. When the consumer has set no
    /// order of its own, page by the id column, which is unique by
    /// definition and therefore a total order.
    ///
    /// Tables whose id is not a real column (a query-sourced view
    /// projecting a synthetic key) are left alone: ordering by a name the
    /// SELECT does not expose would fail the query outright, and an
    /// unstable page beats no page.
    fn ordered_for_paging(&self) -> Table<PostgresDB, E> {
        let mut table = self.table.clone();
        if table.orders().next().is_some() {
            return table;
        }
        let Some(id) = table.id_field().map(|c| c.name().to_string()) else {
            return table;
        };
        if !table.columns().contains_key(&id) {
            return table;
        }
        table.add_order(OrderBy {
            expression: postgres_expr!("{}", (ident(&id))).into(),
            direction: TableSortDirection::Ascending,
        });
        table
    }
}

fn to_cbor_record(record: Record<AnyPostgresType>) -> Record<CborValue> {
    record
        .into_iter()
        .map(|(k, v)| (k, v.into_value()))
        .collect()
}

fn to_native_record(record: &Record<CborValue>) -> Record<AnyPostgresType> {
    record
        .iter()
        .map(|(k, v)| (k.clone(), AnyPostgresType::untyped(v.clone())))
        .collect()
}

#[async_trait]
impl<E> TableShell for PostgresTableShell<E>
where
    E: Entity<AnyPostgresType> + 'static,
{
    fn columns(&self) -> &IndexMap<String, VistaColumn> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, VistaReference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id, to_cbor_record(record)))
            .collect())
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let Some(record) = self.table.get_value(id).await? else {
            return Ok(None);
        };
        Ok(Some(to_cbor_record(record)))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let Some((id, record)) = self.table.get_some_value().await? else {
            return Ok(None);
        };
        Ok(Some((id, to_cbor_record(record))))
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        self.table.get_count().await
    }

    /// Cheap: `Table` clones its query state (conditions/order/pagination)
    /// while the `PostgresDB` connection pool behind it is `Arc`-shared —
    /// the same clone `fetch_window` already does per call.
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(self.clone()))
    }

    fn add_search(&mut self, text: &str) -> Result<()> {
        // Replace-semantics: drop the previous search before pushing the new one.
        if let Some(handle) = self.current_search_handle.take() {
            let _ = self.table.temp_remove_condition(handle);
        }
        let condition = self
            .table
            .data_source()
            .search_table_condition(&self.table, text);
        self.current_search_handle = Some(self.table.temp_add_condition(condition));
        Ok(())
    }

    fn clear_search(&mut self) -> Result<()> {
        if let Some(handle) = self.current_search_handle.take() {
            let _ = self.table.temp_remove_condition(handle);
        }
        Ok(())
    }

    fn set_page_size(&mut self, size: usize) -> Result<()> {
        if size == 0 {
            return Err(error!("page size must be > 0"));
        }
        self.page_size = Some(size);
        Ok(())
    }

    async fn fetch_page(
        &self,
        _vista: &Vista,
        page: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        if page == 0 {
            return Err(error!("page is 1-based; got 0"));
        }
        let size = self
            .page_size
            .ok_or_else(|| error!("set_page_size must be called before fetch_page"))?;

        // Clone the wrapped table so we don't disturb the shell's own
        // condition / order / search state with this call's pagination.
        let mut page_table = self.ordered_for_paging();
        page_table.set_pagination(Some(Pagination::new(page as i64, size as i64)));

        let raw = page_table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id, to_cbor_record(record)))
            .collect())
    }

    async fn fetch_next(
        &self,
        _vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        let size = self
            .page_size
            .ok_or_else(|| error!("set_page_size must be called before fetch_next"))?;

        // Postgres encodes its cursor as the 1-based page number for the next
        // fetch. `None` ⇒ page 1; otherwise the previous call's returned
        // integer.
        let page: i64 = match token {
            None => 1,
            Some(CborValue::Integer(n)) => {
                i64::try_from(n).map_err(|_| error!("fetch_next token out of i64 range"))?
            }
            Some(_) => return Err(error!("invalid fetch_next token type for postgres driver")),
        };
        if page < 1 {
            return Err(error!("fetch_next token must be a 1-based page number"));
        }

        let mut page_table = self.ordered_for_paging();
        page_table.set_pagination(Some(Pagination::new(page, size as i64)));
        let raw = page_table.list_values().await?;
        let records: Vec<(String, Record<CborValue>)> = raw
            .into_iter()
            .map(|(id, record)| (id, to_cbor_record(record)))
            .collect();

        // Exhausted only on an EMPTY page, never on a short one. `records`
        // is keyed by id, so a non-unique id column collapses rows and a
        // full SQL page can arrive here under-length; treating that as the
        // end silently drops every remaining page. The cost of being sure
        // is one extra round trip at the end of a scan.
        let next_token = if records.is_empty() {
            None
        } else {
            Some(CborValue::Integer((page + 1).into()))
        };
        Ok((records, next_token))
    }

    async fn fetch_window(
        &self,
        _vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        // Clone the wrapped table so this call's window doesn't disturb the
        // shell's own condition / order / search state.
        let mut window_table = self.ordered_for_paging();
        window_table.set_pagination(Some(Pagination::window(offset as i64, limit as i64)));

        let raw = window_table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id, to_cbor_record(record)))
            .collect())
    }

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let inserted = self
            .table
            .insert_value(id, &to_native_record(record))
            .await?;
        Ok(to_cbor_record(inserted))
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let replaced = self
            .table
            .replace_value(id, &to_native_record(record))
            .await?;
        Ok(to_cbor_record(replaced))
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let patched = self
            .table
            .patch_value(id, &to_native_record(partial))
            .await?;
        Ok(to_cbor_record(patched))
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        self.table.delete(id).await
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.table.delete_all().await
    }

    async fn insert_vista_return_id_value(
        &self,
        _vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        self.table
            .insert_return_id_value(&to_native_record(record))
            .await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| error!("Unknown column for eq condition", field = field))?
            .clone();
        let sql_value = AnyPostgresType::untyped(value.clone());
        self.table.add_condition(column.eq(sql_value));
        Ok(())
    }

    fn add_op_condition(
        &mut self,
        field: &str,
        op: vantage_vista::FilterOp,
        value: &CborValue,
    ) -> Result<()> {
        use vantage_vista::FilterOp;
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| error!("Unknown column for condition", field = field))?
            .clone();
        match op {
            FilterOp::InSet | FilterOp::NotInSet => {
                let CborValue::Array(items) = value else {
                    return Err(error!(
                        "in_set/not_in_set requires an array value",
                        field = field
                    ));
                };
                let values: Vec<AnyPostgresType> = items
                    .iter()
                    .map(|v| AnyPostgresType::untyped(v.clone()))
                    .collect();
                let condition = match op {
                    FilterOp::InSet => column.in_list(&values),
                    _ => column.not_in_list(&values),
                };
                self.table.add_condition(condition);
            }
            _ => {
                let sql_value = AnyPostgresType::untyped(value.clone());
                let condition = match op {
                    FilterOp::Eq => column.eq(sql_value),
                    FilterOp::Ne => column.ne(sql_value),
                    FilterOp::Gt => column.gt(sql_value),
                    FilterOp::Gte => column.gte(sql_value),
                    FilterOp::Lt => column.lt(sql_value),
                    FilterOp::Lte => column.lte(sql_value),
                    FilterOp::InSet | FilterOp::NotInSet => unreachable!("handled above"),
                };
                self.table.add_condition(condition);
            }
        }
        Ok(())
    }

    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for add_order", field = field));
        }
        // Vista's add_order is replace-semantics — drop any previously-set
        // order before pushing the new one.
        self.table.clear_orders();
        let expr = postgres_expr!("{}", (ident(field)));
        let direction = match dir {
            SortDirection::Ascending => TableSortDirection::Ascending,
            SortDirection::Descending => TableSortDirection::Descending,
        };
        self.table.add_order(OrderBy {
            expression: expr.into(),
            direction,
        });
        Ok(())
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.table.clear_orders();
        Ok(())
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let native_row = to_native_record(row);
        let target = self
            .table
            .get_ref_from_row::<EmptyEntity>(relation, &native_row)?;
        let factory = crate::postgres::vista::factory::PostgresVistaFactory::new(
            self.table.data_source().clone(),
        )
        .with_notify(self.notify_opt_in());
        factory.from_table(target)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        let target = self.table.get_ref_target_erased(relation)?;
        let factory = crate::postgres::vista::factory::PostgresVistaFactory::new(
            self.table.data_source().clone(),
        )
        .with_notify(self.notify_opt_in());
        factory.from_table(target)
    }

    fn get_ref_kinds(&self) -> Vec<(String, vantage_vista::ReferenceKind)> {
        self.table.ref_kinds()
    }

    fn contained(&self) -> &IndexMap<String, ContainedSpec> {
        &self.metadata.contained
    }

    /// Resolve a contained relation. The collection lives in the host column as
    /// JSON (parsed on read, re-serialized on write); the shared
    /// `Table::get_contained_ref` does the rest.
    fn get_contained_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let id_field = self.metadata.id_column.as_deref().unwrap_or("id");
        let parent_id = match row.get(id_field) {
            Some(CborValue::Text(s)) => s.clone(),
            Some(CborValue::Integer(i)) => i128::from(*i).to_string(),
            _ => {
                return Err(error!(
                    "contained traversal requires the parent row's id",
                    relation = relation
                ));
            }
        };
        let db = self.table.data_source().clone();
        let notify = self.notify_opt_in();
        self.table.get_contained_ref(
            relation,
            row,
            parent_id,
            move |t| {
                crate::postgres::vista::factory::PostgresVistaFactory::new(db.clone())
                    .with_notify(notify)
                    .from_table(t)
            },
            parse_json_host,
            |c| CborValue::Text(cbor_to_json(c).to_string()),
        )
    }

    /// Watch the table via Postgres `LISTEN/NOTIFY` and stream a coarse
    /// [`VistaChange::Invalidated`] on every notification.
    ///
    /// Postgres notifications carry no row payload, so this is the
    /// invalidate-and-reconcile end of the spectrum (SurrealDB's LIVE feed emits
    /// the fine-grained variants instead) — the consumer re-reads the set on each
    /// signal. The channel is `{table}_changed` by convention; the application
    /// installs a trigger that `pg_notify`s it on every write (see learn-10's
    /// `db::setup`).
    ///
    /// `LISTEN` succeeds whether or not that trigger exists, and an un-triggered
    /// channel simply never fires — so the capability behind this is opt-in via
    /// [`PostgresVistaFactory::with_notify`](crate::postgres::vista::PostgresVistaFactory::with_notify),
    /// not inferred from the table being writable. Advertised via
    /// [`VistaCapabilities::can_subscribe`].
    async fn watch_vista(&self, _vista: &Vista) -> Result<VistaChangeStream> {
        let channel = format!("{}_changed", self.table.table_name());
        let pool = self.table.data_source().pool().clone();

        let stream = async_stream::try_stream! {
            let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
                .await
                .map_err(|e| error!("open pg listener", details = e.to_string()))?;
            listener
                .listen(&channel)
                .await
                .map_err(|e| error!("LISTEN failed", channel = channel.clone(), details = e.to_string()))?;
            loop {
                listener
                    .recv()
                    .await
                    .map_err(|e| error!("recv notification", details = e.to_string()))?;
                yield VistaChange::Invalidated;
            }
        };
        Ok(Box::pin(stream))
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "postgres"
    }

    /// The SELECT as it stands: every condition, order and page size applied so
    /// far, rendered with values inline. The executed form binds those values as
    /// `$N` parameters instead — same query, different spelling.
    fn preview_query(&self, _vista: &Vista) -> serde_json::Value {
        serde_json::json!({
            "driver": "postgres",
            "table": self.table.table_name(),
            "sql": self.table.select().preview(),
        })
    }
}
