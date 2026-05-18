//! `SqliteTableShell` — owns the typed `Table<SqliteDB, E>` and exposes it
//! through the `TableShell` boundary. The shell is generic in `E` so that
//! `with_expression` closures (parameterized over `E`) survive the wrap;
//! `Vista` erases `E` once at the `Box<dyn TableShell>` boundary.
//!
//! `AnySqliteType` already wraps `ciborium::Value`, so the boundary is a
//! straight unwrap/rewrap. `add_eq_condition` builds a typed
//! `Column<AnySqliteType>::eq` comparison via the `SqliteOperation` trait
//! and pushes it onto the wrapped table.

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
    Column as VistaColumn, Reference as VistaReference, SortDirection, TableShell, Vista,
    VistaCapabilities, VistaMetadata,
};

use crate::primitives::identifier::ident;
use crate::sqlite::SqliteDB;
use crate::sqlite::operation::SqliteOperation;
use crate::sqlite::types::AnySqliteType;

pub struct SqliteTableShell<E = EmptyEntity>
where
    E: Entity<AnySqliteType>,
{
    pub(crate) table: Table<SqliteDB, E>,
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

impl<E> SqliteTableShell<E>
where
    E: Entity<AnySqliteType>,
{
    pub(crate) fn new(
        table: Table<SqliteDB, E>,
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
}

fn to_cbor_record(record: Record<AnySqliteType>) -> Record<CborValue> {
    record
        .into_iter()
        .map(|(k, v)| (k, v.into_value()))
        .collect()
}

fn to_native_record(record: &Record<CborValue>) -> Record<AnySqliteType> {
    record
        .iter()
        .map(|(k, v)| (k.clone(), AnySqliteType::untyped(v.clone())))
        .collect()
}

#[async_trait]
impl<E> TableShell for SqliteTableShell<E>
where
    E: Entity<AnySqliteType> + 'static,
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

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| error!("Unknown column for eq condition", field = field))?
            .clone();
        let sql_value = AnySqliteType::untyped(value.clone());
        self.table.add_condition(column.eq(sql_value));
        Ok(())
    }

    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for add_order", field = field));
        }
        // Vista's add_order is replace-semantics — drop any previously-set
        // order before pushing the new one.
        self.table.clear_orders();
        let expr = sqlite_expr!("{}", (ident(field)));
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
        let mut page_table = self.table.clone();
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

        // SQLite encodes its cursor as the 1-based page number for the next
        // fetch. `None` ⇒ page 1; otherwise the previous call's returned
        // integer.
        let page: i64 = match token {
            None => 1,
            Some(CborValue::Integer(n)) => {
                i64::try_from(n).map_err(|_| error!("fetch_next token out of i64 range"))?
            }
            Some(_) => return Err(error!("invalid fetch_next token type for sqlite driver")),
        };
        if page < 1 {
            return Err(error!("fetch_next token must be a 1-based page number"));
        }

        let mut page_table = self.table.clone();
        page_table.set_pagination(Some(Pagination::new(page, size as i64)));
        let raw = page_table.list_values().await?;
        let records: Vec<(String, Record<CborValue>)> = raw
            .into_iter()
            .map(|(id, record)| (id, to_cbor_record(record)))
            .collect();

        // Exhausted whenever the page returned fewer rows than requested.
        // (Including the empty case — the caller's last call.)
        let next_token = if records.len() == size {
            Some(CborValue::Integer((page + 1).into()))
        } else {
            None
        };
        Ok((records, next_token))
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let native_row = to_native_record(row);
        let target = self
            .table
            .get_ref_from_row::<EmptyEntity>(relation, &native_row)?;
        let factory = crate::sqlite::vista::factory::SqliteVistaFactory::new(
            self.table.data_source().clone(),
        );
        factory.from_table(target)
    }

    fn get_ref_kinds(&self) -> Vec<(String, vantage_vista::ReferenceKind)> {
        self.table.ref_kinds()
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "sqlite"
    }
}
