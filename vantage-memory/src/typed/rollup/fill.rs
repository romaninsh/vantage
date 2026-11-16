//! Reads with roll-ups filled: whole queries, counts and single rows.

use std::collections::HashMap;
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_table::table::Table;
use vantage_types::Entity;
use vantage_vista::FilterOp;

use super::MemoryRollup;
use crate::eval::{MemoryCondition, Query, matches_all, sort_rows, window_rows};
use crate::store::Row;
use crate::store::ids::{id_forms, supplied_id};
use crate::typed::MemoryDB;
use crate::typed::aggregate::rollup_value;
use crate::types::AnyMemoryType;

impl MemoryDB {
    /// Rows matching `q`, with the table's roll-ups filled. When `q` reads a
    /// roll-up column, every row is filled first and `q` runs over them.
    pub(crate) async fn query_rows<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        q: &Query,
    ) -> Result<Vec<(String, Row)>> {
        let store = self.store_table(table).await?;
        let rollups = self.rollups_of(table.table_name());
        if !self.reads_rollups(table.table_name(), q) {
            let mut rows = store.query(q)?;
            self.fill(&rollups, &mut rows).await?;
            return Ok(rows);
        }
        let mut rows = store.query(&Query::new())?;
        self.fill(&rollups, &mut rows).await?;
        let mut matched = Vec::new();
        for (id, row) in rows {
            if matches_all(q, &row)? {
                matched.push((id, row));
            }
        }
        sort_rows(&mut matched, &q.order);
        Ok(window_rows(matched, q))
    }

    /// How many rows match `q`'s conditions and search.
    pub(crate) async fn count_rows<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        q: &Query,
    ) -> Result<usize> {
        if !self.reads_rollups(table.table_name(), q) {
            return self.store_table(table).await?.count(q);
        }
        let all = q.clone().window(0, None);
        Ok(self.query_rows(table, &all).await?.len())
    }

    /// Row `id` of `table` with its roll-ups filled.
    pub(crate) async fn filled_row<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        row: Row,
    ) -> Result<Row> {
        let rollups = self.rollups_of(table.table_name());
        if rollups.is_empty() {
            return Ok(row);
        }
        let mut rows = vec![(id.to_string(), row)];
        self.fill(&rollups, &mut rows).await?;
        Ok(rows.pop().map(|(_, row)| row).unwrap_or_default())
    }

    async fn fill(&self, rollups: &[&MemoryRollup], rows: &mut [(String, Row)]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        // One parent reads only its own children, through an index if any.
        let parent = match rows {
            [(id, _)] => Some(id.clone()),
            _ => None,
        };
        for rollup in rollups {
            let children = self.children(rollup, parent.as_deref()).await?;
            for (id, row) in rows.iter_mut() {
                let cells = children.get(id.as_str()).map_or(&[][..], Vec::as_slice);
                Arc::make_mut(row).insert(rollup.column.clone(), rollup_value(rollup.op, cells));
            }
        }
        Ok(())
    }

    /// The aggregated cells of `rollup`'s child rows, by the parent id their
    /// foreign key holds; only `parent`'s when given. Deferred conditions
    /// (a target's traversal or policy subqueries) resolve here, per read.
    async fn children(
        &self,
        rollup: &MemoryRollup,
        parent: Option<&str>,
    ) -> Result<HashMap<String, Vec<CborValue>>> {
        let Some(live) = self.store.existing_table(&rollup.target) else {
            return Ok(HashMap::new());
        };
        let target = match &self.tx {
            Some(tx) => tx.work_copy(&live).await?,
            None => live,
        };
        let mut q = Query::new();
        for c in &rollup.conditions {
            q = q.filter(c.clone());
        }
        if let Some(id) = parent {
            let forms = CborValue::Array(id_forms(id));
            q = q.filter(MemoryCondition::cmp(
                &rollup.foreign_key,
                FilterOp::InSet,
                forms,
            ));
        }
        let mut out: HashMap<String, Vec<CborValue>> = HashMap::new();
        for (_, row) in target.query(&q.resolve().await?)? {
            let Some(parent) = supplied_id(row.get(&rollup.foreign_key)) else {
                continue;
            };
            let cell = rollup.source.as_ref().and_then(|c| row.get(c)).cloned();
            out.entry(parent)
                .or_default()
                .push(cell.unwrap_or(CborValue::Null));
        }
        Ok(out)
    }
}
