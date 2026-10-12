//! `AggregateShell` — a Vista over an aggregation's output.
//!
//! This is the point of the crate. A derived row set is small and **fully
//! materialised**, so this shell can advertise `Order`, `Search` and `Window`
//! and then actually honour them. Ordering a derived set is ordinary push-down
//! into its own Vista, not a client-side special case layered over partial
//! data — which is exactly the trap the surrounding design is trying to get
//! out of.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, ColumnFlag, Condition, FilterOp, Page, Preview, SortDirection, TableShell,
    VistaCapabilities, VistaMetadata, unsupported,
};

use crate::aggregation::DerivedRows;
use crate::cmp;

/// The rows an aggregation last produced, shared between the engine (which
/// replaces them) and every Vista clone (which reads them).
pub(crate) type SharedRows = Arc<RwLock<DerivedRows>>;

/// Clones share the rows but own their query state, so two differently-sorted
/// views over one aggregate never race.
#[derive(Clone)]
pub(crate) struct AggregateShell {
    rows: SharedRows,
    /// The derived schema; derived rows carry no relations.
    metadata: VistaMetadata,
    capabilities: VistaCapabilities,

    conditions: Vec<(String, CborValue)>,
    order: Option<(String, SortDirection)>,
    search: Option<String>,
}

impl AggregateShell {
    /// Build the shell from the aggregation's declared schema.
    ///
    /// Every derived column is flagged `Orderable` and `Searchable`. That is
    /// not a convenience: the whole derived set is in memory, so there is no
    /// column this shell genuinely cannot order or search by.
    pub(crate) fn new(rows: SharedRows, schema: &[Column]) -> Self {
        let mut metadata = VistaMetadata::new();
        for column in schema {
            let column = column
                .clone()
                .with_flag(ColumnFlag::Orderable)
                .with_flag(ColumnFlag::Searchable);
            if column.has_flag(ColumnFlag::Id) && metadata.id_column.is_none() {
                metadata.id_column = Some(column.name.clone());
            }
            metadata.columns.insert(column.name.clone(), column);
        }

        Self {
            rows,
            metadata,
            // Derived rows are read-only: they are a function of the source,
            // so a write here would have nowhere to land.
            capabilities: [Capability::Order, Capability::Search, Capability::Window]
                .into_iter()
                .collect(),
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

    fn known_column(&self, field: &str) -> Result<()> {
        if self.metadata.columns.contains_key(field) {
            return Ok(());
        }
        Err(error!("unknown column on an aggregate", column = field))
    }
}

#[async_trait]
impl TableShell for AggregateShell {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "aggregate"
    }

    /// An aggregate is derived in memory from the rows its source Dio already
    /// holds, so it issues no query and costs no request. The narrowing is
    /// applied to the derived rows. To see what feeds it, preview the source
    /// table.
    fn preview(&self) -> Preview {
        let mut clauses: Vec<String> = self
            .conditions
            .iter()
            .map(|(field, value)| format!("{field} = {value:?}"))
            .collect();
        if let Some(text) = &self.search {
            clauses.push(format!("search {text:?}"));
        }
        if let Some((column, dir)) = &self.order {
            let dir = match dir {
                SortDirection::Ascending => "asc",
                SortDirection::Descending => "desc",
            };
            clauses.push(format!("order by {column} {dir}"));
        }
        let mut note = "derived in memory from the source Dio's cached rows — no query, no \
                        request. Preview the source table to see what fetches the rows this \
                        reduces."
            .to_string();
        if !clauses.is_empty() {
            note.push_str(" Applied to the derived rows: ");
            note.push_str(&clauses.join(", "));
        }
        Preview::new().with_note(note)
    }

    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        Ok(self.view().into_iter().collect())
    }

    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        Ok(self
            .view()
            .into_iter()
            .find(|(row_id, _)| row_id == id)
            .map(|(_, record)| record))
    }

    async fn count(&self) -> Result<i64> {
        Ok(self.view().len() as i64)
    }

    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        Ok(Page::from_listing(self.view(), offset, limit))
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        if condition.op != FilterOp::Eq {
            return unsupported(Capability::FilterOps);
        }
        self.known_column(&condition.column)?;
        self.conditions
            .push((condition.column.clone(), condition.value.clone()));
        Ok(())
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        if let Some((field, _)) = order {
            self.known_column(field)?;
        }
        self.order = order.map(|(field, dir)| (field.to_string(), dir));
        Ok(())
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.search = text.map(str::to_string);
        Ok(())
    }
}
