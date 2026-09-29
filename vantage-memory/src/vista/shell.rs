//! `TableShell` for `MemoryTableShell`. Live watching lives elsewhere.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{
    Column, FilterOp, Reference, SortDirection, TableShell, Vista, VistaCapabilities,
    VistaChangeStream,
};

use super::{MemoryTableShell, describe, watch};
use crate::eval::matches_all;
use crate::{MemoryCondition, Row};

type Rec = Record<CborValue>;

fn unwrap_rows(rows: Vec<(String, Row)>) -> Vec<(String, Rec)> {
    rows.into_iter()
        .map(|(id, row)| (id, (*row).clone()))
        .collect()
}

#[async_trait]
impl TableShell for MemoryTableShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, Reference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        Some(self.id_column_name())
    }

    async fn list_vista_values(&self, _vista: &Vista) -> Result<IndexMap<String, Rec>> {
        Ok(unwrap_rows(self.table.query(&self.query)?)
            .into_iter()
            .collect())
    }

    async fn get_vista_value(&self, _vista: &Vista, id: &String) -> Result<Option<Rec>> {
        match self.table.get(id) {
            Some(row) if matches_all(&self.query, &row)? => Ok(Some((*row).clone())),
            _ => Ok(None),
        }
    }

    async fn get_vista_some_value(&self, _vista: &Vista) -> Result<Option<(String, Rec)>> {
        let rows = self.table.query(&self.windowed(0, Some(1)))?;
        Ok(unwrap_rows(rows).into_iter().next())
    }

    async fn insert_vista_value(&self, _vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        self.insert_row(id, record)
    }

    async fn replace_vista_value(&self, _vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        self.replace_row(id, record)
    }

    async fn patch_vista_value(&self, _vista: &Vista, id: &String, partial: &Rec) -> Result<Rec> {
        self.patch_row(id, partial)
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        self.delete_row(id)
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.delete_matching()
    }

    async fn insert_vista_return_id_value(&self, _vista: &Vista, record: &Rec) -> Result<String> {
        self.table.insert(record.clone())
    }

    async fn import_vista_values(
        &self,
        _vista: &Vista,
        records: &IndexMap<String, Rec>,
    ) -> Result<usize> {
        Ok(self.import_rows(records))
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        Ok(self.table.count(&self.query)? as i64)
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        self.add_op_condition(field, FilterOp::Eq, value)
    }

    fn add_op_condition(&mut self, field: &str, op: FilterOp, value: &CborValue) -> Result<()> {
        self.query
            .conditions
            .push(MemoryCondition::cmp(field, op, value.clone()));
        Ok(())
    }

    fn set_page_size(&mut self, size: usize) -> Result<()> {
        self.page_size = Some(size);
        Ok(())
    }

    async fn fetch_page(&self, _vista: &Vista, page: usize) -> Result<Vec<(String, Rec)>> {
        if page == 0 {
            return Err(error!("page is 1-based; got 0"));
        }
        let size = self
            .page_size
            .ok_or_else(|| error!("set_page_size must be called before fetch_page"))?;
        let rows = self
            .table
            .query(&self.windowed((page - 1) * size, Some(size)))?;
        Ok(unwrap_rows(rows))
    }

    async fn fetch_window(
        &self,
        _vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Rec)>> {
        Ok(unwrap_rows(
            self.table.query(&self.windowed(offset, Some(limit)))?,
        ))
    }

    async fn fetch_window_counted(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<(String, Rec)>, Option<i64>)> {
        let rows = self.fetch_window(vista, offset, limit).await?;
        Ok((rows, Some(self.table.count(&self.query)? as i64)))
    }

    fn add_search(&mut self, text: &str) -> Result<()> {
        self.query.search = Some(text.to_string());
        Ok(())
    }

    fn clear_search(&mut self) -> Result<()> {
        self.query.search = None;
        Ok(())
    }

    /// Replaces any previous order with the single key `field`.
    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        self.query.order = vec![(field.to_string(), dir)];
        Ok(())
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.query.order.clear();
        Ok(())
    }

    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(self.clone()))
    }

    fn get_ref(&self, relation: &str, row: &Rec) -> Result<Vista> {
        self.traverse(relation, row)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        self.ref_target(relation)
    }

    fn driver_name(&self) -> &'static str {
        "memory"
    }

    fn preview_query(&self, _vista: &Vista) -> serde_json::Value {
        let order: Vec<serde_json::Value> = self
            .query
            .order
            .iter()
            .map(|(field, dir)| {
                let dir = match dir {
                    SortDirection::Ascending => "asc",
                    SortDirection::Descending => "desc",
                };
                serde_json::json!([field, dir])
            })
            .collect();
        serde_json::json!({
            "driver": "memory",
            "table": self.table.name(),
            "conditions": self.query.conditions.iter().map(describe).collect::<Vec<_>>(),
            "search": self.query.search,
            "order": order,
        })
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    async fn watch_vista(&self, _vista: &Vista) -> Result<VistaChangeStream> {
        Ok(watch::stream(self.table().clone(), self.query().clone()))
    }
}
