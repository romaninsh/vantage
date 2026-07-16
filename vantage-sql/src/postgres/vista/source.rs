//! `PostgresTableShell` — owns the typed `Table<PostgresDB, E>` and exposes
//! it through the `TableShell` boundary. The shell is generic in `E` so that
//! `with_expression` closures (parameterized over `E`) survive the wrap;
//! `Vista` erases `E` once at the `Box<dyn TableShell>` boundary.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::pagination::Pagination;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{
    Column as VistaColumn, ContainedSpec, Reference as VistaReference, SortDirection, TableShell,
    Vista, VistaCapabilities, VistaMetadata,
};

use crate::postgres::PostgresDB;
use crate::postgres::operation::PostgresOperation;
use crate::primitives::identifier::ident;
use crate::postgres::types::AnyPostgresType;
use crate::types::{cbor_to_json, parse_json_host};

pub struct PostgresTableShell<E = EmptyEntity>
where
    E: Entity<AnyPostgresType>,
{
    pub(crate) table: Table<PostgresDB, E>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
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
        }
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

    async fn fetch_window(
        &self,
        _vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        // Clone the wrapped table so this call's window doesn't disturb the
        // shell's own condition / order / search state.
        let mut window_table = self.table.clone();
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
        );
        factory.from_table(target)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        let target = self.table.get_ref_target::<EmptyEntity>(relation)?;
        let factory = crate::postgres::vista::factory::PostgresVistaFactory::new(
            self.table.data_source().clone(),
        );
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
        self.table.get_contained_ref(
            relation,
            row,
            parent_id,
            move |t| {
                crate::postgres::vista::factory::PostgresVistaFactory::new(db.clone()).from_table(t)
            },
            parse_json_host,
            |c| CborValue::Text(cbor_to_json(c).to_string()),
        )
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "postgres"
    }
}
