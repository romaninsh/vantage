//! `AggregateShell` — a Vista over an aggregation's output.
//!
//! This is the point of the module. A derived row set is small and **fully
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
    VistaCapabilities, VistaMetadata, VistaQuery, condition_column_error, unsupported,
};

use crate::aggregate::aggregation::DerivedRows;

/// The rows an aggregation last produced, shared between the engine (which
/// replaces them) and every Vista clone (which reads them).
pub(crate) type AggregateRows = Arc<RwLock<DerivedRows>>;

/// Clones share the rows but own their query state, so two differently-sorted
/// views over one aggregate never race.
#[derive(Clone)]
pub(crate) struct AggregateShell {
    rows: AggregateRows,
    /// The derived schema; derived rows carry no relations.
    metadata: VistaMetadata,
    capabilities: VistaCapabilities,
    /// This clone's conditions, search and order, evaluated by vista's
    /// in-memory evaluator.
    query: VistaQuery,
}

impl AggregateShell {
    /// Build the shell from the aggregation's declared schema.
    ///
    /// Every derived column is flagged `Orderable` and `Searchable`. That is
    /// not a convenience: the whole derived set is in memory, so there is no
    /// column this shell genuinely cannot order or search by.
    pub(crate) fn new(rows: AggregateRows, schema: &[Column]) -> Self {
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
            query: VistaQuery::new(),
        }
    }

    /// Apply this clone's conditions, search and order to the current rows.
    fn view(&self) -> Result<Vec<(String, Record<CborValue>)>> {
        let mut rows = Vec::new();
        for (id, record) in &self.rows.read().expect("aggregate rows poisoned").rows {
            if self.query.matches(record, &self.metadata)? {
                rows.push((id.clone(), record.clone()));
            }
        }
        self.query.sort_rows(&mut rows, &self.metadata);
        Ok(rows)
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
            .query
            .conditions
            .iter()
            .map(|c| format!("{} = {:?}", c.column, c.value))
            .collect();
        if let Some(text) = &self.query.search {
            clauses.push(format!("search {text:?}"));
        }
        if let Some((column, dir)) = &self.query.order {
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
        Ok(self.view()?.into_iter().collect())
    }

    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        Ok(self
            .view()?
            .into_iter()
            .find(|(row_id, _)| row_id == id)
            .map(|(_, record)| record))
    }

    async fn count(&self) -> Result<i64> {
        Ok(self.view()?.len() as i64)
    }

    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        Ok(Page::from_listing(self.view()?, offset, limit))
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        if condition.op != FilterOp::Eq {
            return unsupported(Capability::FilterOps);
        }
        if !self.metadata.columns.contains_key(&condition.column) {
            return Err(condition_column_error(&self.metadata, &condition.column));
        }
        self.query.conditions.push(condition.clone());
        Ok(())
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        if let Some((field, _)) = order {
            self.known_column(field)?;
        }
        self.query.order = order.map(|(field, dir)| (field.to_string(), dir));
        Ok(())
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.query.search = text.map(str::to_string);
        Ok(())
    }
}
