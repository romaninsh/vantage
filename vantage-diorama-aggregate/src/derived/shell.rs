//! `AggregateShell` — a Vista over an aggregation's output.
//!
//! This is the point of the crate. A derived row set is small and **fully
//! materialised**, so this shell can advertise `can_order`, `can_search` and
//! `can_count` and then actually honour them. Ordering a derived set is
//! ordinary push-down into its own Vista, not a client-side special case
//! layered over partial data — which is exactly the trap the surrounding
//! design is trying to get out of.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{
    Column, Reference, SortDirection, TableShell, Vista, VistaCapabilities, flags,
};

use crate::aggregation::DerivedRows;
use crate::cmp;

/// The rows an aggregation last produced, shared between the engine (which
/// replaces them) and every Vista clone (which reads them).
pub(crate) type SharedRows = Arc<RwLock<DerivedRows>>;

pub(crate) struct AggregateShell {
    rows: SharedRows,
    columns: IndexMap<String, Column>,
    references: IndexMap<String, Reference>,
    id_column: Option<String>,
    capabilities: VistaCapabilities,

    // Per-clone query state. `clone_shell` gives each narrowing its own copy,
    // so two differently-sorted views over one aggregate never race.
    conditions: Vec<(String, CborValue)>,
    order: Option<(String, SortDirection)>,
    search: Option<String>,
}

impl AggregateShell {
    /// Build the shell from the aggregation's declared schema.
    ///
    /// Every derived column is flagged `ORDERABLE` and `SEARCHABLE`. That is
    /// not a convenience: the whole derived set is in memory, so there is no
    /// column this shell genuinely cannot order or search by.
    pub(crate) fn new(rows: SharedRows, schema: &[Column]) -> Self {
        let mut columns = IndexMap::new();
        let mut id_column = None;
        for column in schema {
            let mut column = column.clone();
            if !column.has_flag(flags::ORDERABLE) {
                column = column.with_flag(flags::ORDERABLE);
            }
            if !column.has_flag(flags::SEARCHABLE) {
                column = column.with_flag(flags::SEARCHABLE);
            }
            if column.has_flag(flags::ID) && id_column.is_none() {
                id_column = Some(column.name.clone());
            }
            columns.insert(column.name.clone(), column);
        }

        Self {
            rows,
            columns,
            references: IndexMap::new(),
            id_column,
            capabilities: VistaCapabilities {
                can_count: true,
                can_order: true,
                can_search: true,
                can_fetch_window: true,
                // Derived rows are read-only: they are a function of the
                // source, so a write here would have nowhere to land.
                can_insert: false,
                can_update: false,
                can_delete: false,
                ..Default::default()
            },
            conditions: Vec::new(),
            order: None,
            search: None,
        }
    }

    /// Apply this clone's conditions, search and order to the current rows.
    fn view(&self) -> Vec<(String, Record<CborValue>)> {
        let guard = self.rows.read().expect("aggregate rows poisoned");
        let mut rows: Vec<(String, Record<CborValue>)> = guard
            .rows
            .iter()
            .filter(|(_, record)| {
                self.conditions.iter().all(|(field, expected)| {
                    cmp::column(record, field).is_some_and(|actual| actual == expected)
                })
            })
            .filter(|(_, record)| match &self.search {
                Some(needle) => cmp::matches_search(record, needle),
                None => true,
            })
            .cloned()
            .collect();
        drop(guard);

        if let Some((field, direction)) = &self.order {
            let descending = matches!(direction, SortDirection::Descending);
            rows.sort_by(|(a_id, a), (b_id, b)| {
                cmp::compare_rows((a_id, a), (b_id, b), field, descending)
            });
        }
        rows
    }
}

#[async_trait]
impl TableShell for AggregateShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        &self.columns
    }

    fn references(&self) -> &IndexMap<String, Reference> {
        &self.references
    }

    fn id_column(&self) -> Option<&str> {
        self.id_column.as_deref()
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "aggregate"
    }

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        Ok(self.view().into_iter().collect())
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let guard = self.rows.read().expect("aggregate rows poisoned");
        Ok(guard
            .rows
            .iter()
            .find(|(row_id, _)| row_id == id)
            .map(|(_, record)| record.clone()))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        Ok(self.view().into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        Ok(self.view().len() as i64)
    }

    async fn fetch_window(
        &self,
        _vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        Ok(self.view().into_iter().skip(offset).take(limit).collect())
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        if !self.columns.contains_key(field) {
            return Err(error!("unknown column on an aggregate", column = field));
        }
        self.conditions.push((field.to_string(), value.clone()));
        Ok(())
    }

    /// Replace semantics, matching `Vista::add_order`.
    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        if !self.columns.contains_key(field) {
            return Err(error!("unknown column on an aggregate", column = field));
        }
        self.order = Some((field.to_string(), dir));
        Ok(())
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.order = None;
        Ok(())
    }

    fn add_search(&mut self, text: &str) -> Result<()> {
        self.search = Some(text.to_string());
        Ok(())
    }

    fn clear_search(&mut self) -> Result<()> {
        self.search = None;
        Ok(())
    }

    /// Clones share the rows but get their own query state — the property that
    /// lets two views sort one aggregate differently without interfering.
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(Self {
            rows: self.rows.clone(),
            columns: self.columns.clone(),
            references: self.references.clone(),
            id_column: self.id_column.clone(),
            capabilities: self.capabilities.clone(),
            conditions: self.conditions.clone(),
            order: self.order.clone(),
            search: self.search.clone(),
        }))
    }
}
