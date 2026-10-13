//! How [`MockShell`] resolves its references: joins to registered target
//! stores, contained relations from the parent row.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::{Result, error};

use super::{MockShell, Rec};
use crate::{
    contained::{ContainedWriteback, build_contained_vista},
    reference::Reference,
    vista::Vista,
};

impl MockShell {
    /// The registered target of join `reference`, narrowed by its keys
    /// against `row` through the Vista, so the target's `query()` records
    /// the join. No read happens, so this succeeds while the target's reads
    /// fail.
    pub(super) fn join_ref(&self, reference: &Reference, row: Option<&Rec>) -> Result<Vista> {
        let relation = reference.name.as_str();
        let target =
            self.ref_targets.get(relation).cloned().ok_or_else(|| {
                error!("no ref target registered for relation", relation = relation)
            })?;
        let target = Vista::new(reference.target.clone(), target);
        match row {
            Some(row) => reference.narrow(target, row),
            None => Ok(target),
        }
    }

    /// Serve contained relation `reference` from `row`'s host column, writing
    /// changes back into the parent record in this store. Without a row, the
    /// bare target: the contained schema with no records and no writes.
    pub(super) fn contained_ref(&self, reference: &Reference, row: Option<&Rec>) -> Result<Vista> {
        let relation = reference.name.as_str();
        let schema = self.contained.get(relation).cloned().unwrap_or_default();
        let Some(row) = row else {
            return build_contained_vista(reference, schema, None, None, None);
        };
        let host_column = reference.host_column.clone().unwrap_or_default();
        let parent_id = match row.get(self.id_field()) {
            Some(CborValue::Text(s)) => self.key(s),
            _ => {
                return Err(error!(
                    "contained traversal requires the parent row's id",
                    relation = relation
                ));
            }
        };
        let host_value = row.get(&host_column).cloned();
        let data = self.data.clone();
        let writeback: ContainedWriteback = Arc::new(move |collection: CborValue| {
            let (data, host_column, parent_id) =
                (data.clone(), host_column.clone(), parent_id.clone());
            Box::pin(async move {
                if let Some(record) = data.lock().unwrap().get_mut(&parent_id) {
                    record.insert(host_column, collection);
                }
                Ok(())
            })
        });
        build_contained_vista(
            reference,
            schema,
            host_value.as_ref(),
            Some(writeback),
            None,
        )
    }

    /// The narrowing a real driver would translate: `where … order by …
    /// search …`, or `all rows`.
    pub(super) fn render_query(&self) -> String {
        let mut parts = Vec::new();
        let query = &self.query;
        if !query.conditions.is_empty() {
            let filters: Vec<String> = query
                .conditions
                .iter()
                .map(|c| format!("{} {} {:?}", c.column, c.op.key_symbol(), c.value))
                .collect();
            parts.push(format!("where {}", filters.join(" and ")));
        }
        if let Some((column, dir)) = &query.order {
            let dir = match dir {
                crate::SortDirection::Ascending => "asc",
                crate::SortDirection::Descending => "desc",
            };
            parts.push(format!("order by {column} {dir}"));
        }
        if let Some(text) = &query.search {
            parts.push(format!("search {text:?}"));
        }
        if parts.is_empty() {
            parts.push("all rows".to_string());
        }
        parts.join(" ")
    }
}
