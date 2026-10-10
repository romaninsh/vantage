//! `MysqlTableShell` — owns the typed `Table<MysqlDB, E>` and exposes it
//! through the `TableShell` boundary. The shell is generic in `E` so that
//! `with_expression` closures (parameterized over `E`) survive the wrap;
//! `Vista` erases `E` once at the `Box<dyn TableShell>` boundary.

mod import;
#[cfg(feature = "rhai")]
mod scripted;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_expressions::Expression;
use vantage_table::conditions::ConditionHandle;
use vantage_table::pagination::Pagination;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{
    Column as VistaColumn, ContainedSpec, Reference as VistaReference, SortDirection, TableShell,
    Vista, VistaCapabilities, VistaMetadata,
};

use crate::condition::MysqlCondition;
use crate::mysql::MysqlDB;
use crate::mysql::operation::MysqlOperation;
use crate::mysql::types::AnyMysqlType;
use crate::mysql::vista::factory::{MysqlSpecResolver, MysqlVistaFactory};
use crate::primitives::identifier::ident;
use crate::types::{cbor_to_json, parse_json_host};

#[derive(Clone)]
pub struct MysqlTableShell<E = EmptyEntity>
where
    E: Entity<AnyMysqlType>,
{
    pub(crate) table: Table<MysqlDB, E>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// Handle for the active quicksearch condition (if any). Used by
    /// `clear_search` and by `add_search`'s replace-semantics to remove the
    /// previous search before pushing the new one.
    pub(crate) current_search_handle: Option<ConditionHandle>,
    /// Rows per page declared via `set_page_size`. `None` until the consumer
    /// declares it; `fetch_page` and `fetch_next` error in that case.
    pub(crate) page_size: Option<usize>,
    /// Spec resolver handed to the factories that build traversal targets.
    pub(crate) resolver: Option<MysqlSpecResolver>,
}

impl<E> MysqlTableShell<E>
where
    E: Entity<AnyMysqlType>,
{
    pub(crate) fn new(
        table: Table<MysqlDB, E>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
        resolver: Option<MysqlSpecResolver>,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
            current_search_handle: None,
            page_size: None,
            resolver,
        }
    }

    /// A factory over this table's data source, carrying the spec resolver.
    fn factory(&self) -> MysqlVistaFactory {
        let factory = MysqlVistaFactory::new(self.table.data_source().clone());
        match &self.resolver {
            Some(resolver) => factory.with_resolver(resolver.clone()),
            None => factory,
        }
    }

    /// Give a paged read a deterministic order.
    ///
    /// `LIMIT`/`OFFSET` without `ORDER BY` is not a stable window, and a sort
    /// on a non-unique column leaves ties in unspecified order, so successive
    /// pages could repeat or skip rows. The id column is unique, so it is
    /// appended as the final tiebreak (or becomes the whole order when the
    /// consumer set none).
    ///
    /// Tables without an id column are left alone.
    fn ordered_for_paging(&self) -> Table<MysqlDB, E> {
        let mut table = self.table.clone();
        let Some(id) = table.id_field().map(|c| c.name().to_string()) else {
            return table;
        };
        table.add_order(OrderBy {
            expression: crate::mysql_expr!("{}", (ident(&id))).into(),
            direction: TableSortDirection::Ascending,
        });
        table
    }

    /// List rows through a clone of the wrapped table so this call's
    /// pagination doesn't disturb the shell's own query state.
    async fn list_paginated(
        &self,
        pagination: Pagination,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        let mut table = self.ordered_for_paging();
        table.set_pagination(Some(pagination));
        let raw = table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id, to_cbor_record(record)))
            .collect())
    }
}

fn to_cbor_record(record: Record<AnyMysqlType>) -> Record<CborValue> {
    record
        .into_iter()
        .map(|(k, v)| (k, v.into_value()))
        .collect()
}

fn to_native_record(record: &Record<CborValue>) -> Record<AnyMysqlType> {
    record
        .iter()
        .map(|(k, v)| (k.clone(), AnyMysqlType::untyped(v.clone())))
        .collect()
}

#[async_trait]
impl<E> TableShell for MysqlTableShell<E>
where
    E: Entity<AnyMysqlType> + 'static,
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

    async fn import_vista_values(
        &self,
        _vista: &Vista,
        records: &IndexMap<String, Record<CborValue>>,
    ) -> Result<usize> {
        if !self.capabilities.can_import {
            return Err(self.default_error("import_vista_values", "can_import"));
        }
        import::import(self, records).await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| error!("Unknown column for eq condition", field = field))?
            .clone();
        let sql_value = AnyMysqlType::untyped(value.clone());
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
                let values: Vec<AnyMysqlType> = items
                    .iter()
                    .map(|v| AnyMysqlType::untyped(v.clone()))
                    .collect();
                let condition = match op {
                    FilterOp::InSet => column.in_list(&values),
                    _ => column.not_in_list(&values),
                };
                self.table.add_condition(condition);
            }
            // The same `LIKE '%…%' ESCAPE '$'` quicksearch uses, on one column.
            FilterOp::Like => {
                let pattern = crate::like_pattern(value);
                self.table.add_condition(crate::mysql_expr!(
                    "{} LIKE {} ESCAPE '$'",
                    (crate::primitives::identifier::ident(field)),
                    pattern
                ));
            }
            _ => {
                let sql_value = AnyMysqlType::untyped(value.clone());
                let condition = match op {
                    FilterOp::Eq => column.eq(sql_value),
                    FilterOp::Ne => column.ne(sql_value),
                    FilterOp::Gt => column.gt(sql_value),
                    FilterOp::Gte => column.gte(sql_value),
                    FilterOp::Lt => column.lt(sql_value),
                    FilterOp::Lte => column.lte(sql_value),
                    FilterOp::InSet | FilterOp::NotInSet | FilterOp::Like => {
                        unreachable!("handled above")
                    }
                };
                self.table.add_condition(condition);
            }
        }
        Ok(())
    }

    /// Accepts a [`MysqlCondition`] or the `Expression<AnyMysqlType>` the Rhai
    /// vocabulary's expression constructors produce.
    fn add_raw_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        let condition = match condition.downcast::<MysqlCondition>() {
            Ok(condition) => *condition,
            Err(other) => match other.downcast::<Expression<AnyMysqlType>>() {
                Ok(expr) => MysqlCondition::from(*expr),
                Err(_) => {
                    return Err(error!(
                        "add_raw_condition expected a MySQL condition expression",
                        source_type = std::any::type_name::<Self>()
                    ));
                }
            },
        };
        self.table.add_condition(condition);
        Ok(())
    }

    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for add_order", field = field));
        }
        // Replace-semantics: drop any previously-set order first.
        self.table.clear_orders();
        let direction = match dir {
            SortDirection::Ascending => TableSortDirection::Ascending,
            SortDirection::Descending => TableSortDirection::Descending,
        };
        self.table.add_order(OrderBy {
            expression: crate::mysql_expr!("{}", (ident(field))).into(),
            direction,
        });
        Ok(())
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.table.clear_orders();
        Ok(())
    }

    /// Cheap: `Table` clones its query state (conditions/order/pagination)
    /// while the connection pool behind it is shared.
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
        self.list_paginated(Pagination::new(page as i64, size as i64))
            .await
    }

    async fn fetch_window(
        &self,
        _vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        self.list_paginated(Pagination::window(offset as i64, limit as i64))
            .await
    }

    async fn fetch_next(
        &self,
        _vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        let size = self
            .page_size
            .ok_or_else(|| error!("set_page_size must be called before fetch_next"))?;

        // The cursor is the 1-based page number of the next fetch; `None` is
        // page 1.
        let page: i64 = match token {
            None => 1,
            Some(CborValue::Integer(n)) => {
                i64::try_from(n).map_err(|_| error!("fetch_next token out of i64 range"))?
            }
            Some(_) => return Err(error!("invalid fetch_next token type for mysql driver")),
        };
        if page < 1 {
            return Err(error!("fetch_next token must be a 1-based page number"));
        }

        let records = self
            .list_paginated(Pagination::new(page, size as i64))
            .await?;

        // A short page (including an empty one) means the data is exhausted.
        let next_token = (records.len() == size).then(|| CborValue::Integer((page + 1).into()));
        Ok((records, next_token))
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        // A reference carrying a Rhai `build_script` resolves through the script
        // engine instead of the fixed FK eq-condition path below.
        #[cfg(feature = "rhai")]
        if let Some(script) = self
            .metadata
            .references
            .get(relation)
            .and_then(|r| r.build_script.as_deref())
        {
            return self.get_ref_via_script(script, row);
        }

        let native_row = to_native_record(row);
        let target = self
            .table
            .get_ref_from_row::<EmptyEntity>(relation, &native_row)?;
        self.factory().from_table(target)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        let target = self.table.get_ref_target_erased(relation)?;
        self.factory().from_table(target)
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
        let factory = self.factory();
        self.table.get_contained_ref(
            relation,
            row,
            parent_id,
            move |t| factory.from_table(t),
            parse_json_host,
            |c| CborValue::Text(cbor_to_json(c).to_string()),
        )
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "mysql"
    }

    /// The SELECT as it stands: every condition, order and page size applied so
    /// far, rendered with values inline. The executed form binds those values as
    /// `?` parameters instead — same query, different spelling.
    fn preview_query(&self, _vista: &Vista) -> serde_json::Value {
        serde_json::json!({
            "driver": "mysql",
            "table": self.table.table_name(),
            "sql": self.table.select().preview(),
        })
    }

    /// The MySQL query vocabulary (`select`, `ident`, `expr`, operators, …)
    /// plus `self.with_condition(<expr>)`, which pushes the expression through
    /// [`add_raw_condition`](Self::add_raw_condition).
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut vantage_rhai::rhai::Engine) {
        scripted::register_extensions(engine);
    }
}
