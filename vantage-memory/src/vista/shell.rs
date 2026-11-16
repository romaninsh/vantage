//! `TableShell` for `MemoryTableShell`: reads and writes go through the
//! typed table, so its hooks, invariants and transaction apply. Cells are
//! conformed to their column's affinity before they reach it.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Context, Result, error};
use vantage_dataset::invariants::conform;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::pagination::Pagination;
use vantage_table::sorting::SortDirection as TableSort;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Condition, Page, Preview, SortDirection, TableShell, Transaction, Vista, VistaCapabilities,
    VistaChangeStream, VistaMetadata,
};

use super::affinity::conform_record;
use super::{MemoryTableShell, watch};
use crate::store::Row;
use crate::typed::convert::{from_cbor_record, table_query, to_cbor_record};
use crate::{AnyMemoryType, MemoryDB};

type Rec = Record<CborValue>;

fn cbor_rows<C: FromIterator<(String, Rec)>>(rows: IndexMap<String, Record<AnyMemoryType>>) -> C {
    rows.into_iter()
        .map(|(id, row)| (id, to_cbor_record(&row)))
        .collect()
}

impl MemoryTableShell {
    /// `record` with every cell in its stored form (see `affinity`), so a
    /// written row compares like a seeded one.
    fn stored_form(&self, record: &Rec) -> Rec {
        let mut row = record.clone();
        conform_record(&self.metadata, self.id_column_name(), &mut row);
        row
    }

    fn native(&self, record: &Rec) -> Record<AnyMemoryType> {
        from_cbor_record(&self.stored_form(record))
    }

    /// The row just inserted under `id`, as an insert with an id returns it:
    /// stored, roll-ups filled, no read hooks.
    async fn inserted(&self, id: &str) -> Result<Rec> {
        let db = self.table.data_source();
        let row = db.store_table(&self.table).await?.get(id).ok_or_else(|| {
            error!("Row not found", table = self.handle.name(), id = id).mark_not_found()
        })?;
        Ok(to_cbor_record(&db.returned(&self.table, id, row).await?))
    }

    /// Every record whose id is new, through the table. Nothing is written
    /// unless every record is: outside a transaction the import runs in an
    /// unclaimed one of its own, committed once every insert succeeded, so
    /// it never waits on a transaction holding the store.
    async fn import_rows(&self, records: &IndexMap<String, Rec>) -> Result<usize> {
        let db = self.table.data_source();
        if db.transaction().is_some() {
            return self.import_into(&self.table, records).await;
        }
        if records.keys().all(|id| self.handle.get(id).is_some()) {
            return Ok(0);
        }
        let tx = Transaction::new();
        let table = self
            .table
            .with_data_source(db.in_unclaimed_transaction(&tx)?);
        match self.import_into(&table, records).await {
            Ok(inserted) => {
                tx.commit().await?;
                Ok(inserted)
            }
            Err(e) => match tx.rollback().await {
                Ok(_) => Err(e),
                Err(rollback) => Err::<usize, _>(rollback).context(e),
            },
        }
    }

    /// The import on `table`; a record that doesn't fit the set fails it
    /// before anything is written.
    async fn import_into(
        &self,
        table: &Table<MemoryDB, EmptyEntity>,
        records: &IndexMap<String, Rec>,
    ) -> Result<usize> {
        let db = table.data_source();
        let invariants: IndexMap<String, CborValue> = table
            .invariants()
            .iter()
            .map(|(k, v)| (k.clone(), v.value().clone()))
            .collect();
        let fitted = db.settle(
            async {
                let store = db.store_table(table).await?;
                let mut fitted = Vec::new();
                for (id, record) in records.iter().filter(|(id, _)| store.get(id).is_none()) {
                    let mut row = self.stored_form(record);
                    conform(&mut row, &invariants)?;
                    row.insert(
                        self.id_column_name().to_string(),
                        CborValue::Text(id.clone()),
                    );
                    // Checked with its roll-ups filled, as insert does.
                    if db.fit(table, id, Row::new(row.clone())).await?.is_none() {
                        return Err(MemoryDB::misfit(self.handle.name(), id));
                    }
                    fitted.push((id, row));
                }
                Ok(fitted)
            }
            .await,
        )?;
        for (id, row) in &fitted {
            table
                .insert_value(id.to_string(), &from_cbor_record(row))
                .await?;
        }
        Ok(fitted.len())
    }

    /// The query rendered for `preview`.
    fn render_query(&self) -> String {
        let mut out = format!("scan {}", self.handle.name());
        let clauses: Vec<String> = self.table.conditions().map(|c| c.describe()).collect();
        if !clauses.is_empty() {
            out.push_str(" WHERE ");
            out.push_str(&clauses.join(" AND "));
        }
        let keys: Vec<String> = self
            .table
            .orders()
            .map(|(key, dir)| match dir {
                TableSort::Ascending => format!("{} ASC", key.describe()),
                TableSort::Descending => format!("{} DESC", key.describe()),
            })
            .collect();
        if !keys.is_empty() {
            out.push_str(" ORDER BY ");
            out.push_str(&keys.join(", "));
        }
        out
    }
}

#[async_trait]
impl TableShell for MemoryTableShell {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "memory"
    }

    fn preview(&self) -> Preview {
        Preview::new().with_query(self.render_query())
    }

    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        Ok(cbor_rows(self.table.list_values().await?))
    }

    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        let row = self.table.get_value(id.to_string()).await?;
        Ok(row.map(|row| to_cbor_record(&row)))
    }

    async fn count(&self) -> Result<i64> {
        self.table.check_read().await?;
        self.table.data_source().get_table_count(&self.table).await
    }

    /// The store counts the set as cheaply as it cuts the window, so the
    /// page always carries the total.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let mut window = self.table.clone();
        window.set_pagination(Some(Pagination::window(offset as i64, limit as i64)));
        let rows: Vec<(String, Rec)> = cbor_rows(window.list_values().await?);
        Ok(Page::new(rows).with_total(self.count().await?))
    }

    /// A table with roll-ups hears its own writes and those of the tables
    /// its roll-ups read (the ones the store holds when the watch starts),
    /// each as `Invalidated`.
    async fn watch(&self) -> Result<VistaChangeStream> {
        self.table.check_read().await?;
        let db = self.table.data_source();
        let targets = db.rollup_targets(self.table.table_name())?;
        if !targets.is_empty() {
            let tables: Vec<_> = std::iter::once(self.handle.clone())
                .chain(targets.iter().filter_map(|t| db.store().existing_table(t)))
                .collect();
            return Ok(watch::invalidations(&tables));
        }
        let query = table_query(db, &self.table).await?;
        Ok(watch::stream(self.handle.clone(), query))
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.narrow_by(condition)
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.replace_search(text)
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        self.replace_order(order)
    }

    fn add_order(&mut self, column: &str, dir: SortDirection) -> Result<()> {
        self.push_order(column, dir)
    }

    async fn insert(&self, id: Option<&str>, record: &Rec) -> Result<(String, Rec)> {
        let native = self.native(record);
        let Some(id) = id else {
            let id = self.table.insert_return_id_value(&native).await?;
            let inserted = self.inserted(&id).await?;
            return Ok((id, inserted));
        };
        let inserted = self.table.insert_value(id.to_string(), &native).await?;
        Ok((id.to_string(), to_cbor_record(&inserted)))
    }

    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        let native = self.native(record);
        let replaced = self.table.replace_value(id.to_string(), &native).await?;
        Ok(to_cbor_record(&replaced))
    }

    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        let native = self.native(record);
        let upserted = self.table.upsert_value(id.to_string(), &native).await?;
        Ok(to_cbor_record(&upserted))
    }

    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        let native = self.native(partial);
        let patched = self.table.patch_value(id.to_string(), &native).await?;
        Ok(to_cbor_record(&patched))
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.table.delete(id.to_string()).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.table.delete_all().await
    }

    async fn import(&self, rows: &IndexMap<String, Rec>) -> Result<usize> {
        self.import_rows(rows).await
    }

    fn get_ref(&self, relation: &str, row: Option<&Rec>) -> Result<Vista> {
        self.traverse(relation, row)
    }

    /// The table keeps its hooks, conditions and references; only its
    /// datasource joins `tx`, claiming nothing until the first read or write.
    fn rebind(&self, tx: &Transaction) -> Result<Box<dyn TableShell>> {
        let db = self.table.data_source().in_transaction(tx)?;
        Ok(Box::new(Self {
            table: self.table.with_data_source(db),
            ..self.clone()
        }))
    }
}
