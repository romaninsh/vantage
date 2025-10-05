//! Query execution methods for ReDB

use redb::{ReadableTable, TableDefinition};

use super::core::Redb;
use crate::util::{Context, Result, vantage_error};

impl Redb {
    async fn get_all_records<E>(
        &self,
        table_name: &str,
        limit: Option<usize>,
        skip: usize,
    ) -> Result<Vec<E>>
    where
        E: vantage_core::Entity,
    {
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table_name);
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(table_def)?;

        let mut results = Vec::new();
        let mut count = 0;

        for (i, item) in table.iter()?.enumerate() {
            if i < skip {
                continue;
            }
            if let Some(limit) = limit
                && count >= limit
            {
                break;
            }

            let (_id, data) = item?;
            let entity = bincode::deserialize::<E>(data.value())?;
            results.push(entity);
            count += 1;
        }

        Ok(results)
    }

    async fn get_records_by_ids<E>(&self, table_name: &str, ids: Vec<String>) -> Result<Vec<E>>
    where
        E: vantage_core::Entity,
    {
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table_name);
        let read_txn = self.begin_read()?;
        let table = read_txn.open_table(table_def)?;

        let mut results = Vec::new();

        for id in ids {
            let data = table.get(id.as_str())?;
            let data_access =
                data.ok_or_else(|| vantage_error!("Record with id '{}' not found", id))?;
            let entity = bincode::deserialize::<E>(data_access.value())?;
            results.push(entity);
        }

        Ok(results)
    }

    async fn get_ids_by_condition(
        &self,
        table_name: &str,
        column: &str,
        value: &serde_json::Value,
    ) -> Result<Vec<String>> {
        let index_table_name = format!("{}_by_{}", table_name, column);
        let index_table_def: TableDefinition<&str, &str> = TableDefinition::new(&index_table_name);
        let read_txn = self.begin_read()?;

        // Try to open index table, fail if it doesn't exist
        let index_table = read_txn
            .open_table(index_table_def)
            .with_context(|| format!("Index table '{}_by_{}' not found", table_name, column))?;

        // Encode value as JSON string for index lookup
        let lookup_key = serde_json::to_string(value).with_context(|| {
            format!(
                "Failed to encode value as JSON for index lookup: {:?}",
                value
            )
        })?;

        // Look up in index table
        match index_table.get(lookup_key.as_str())? {
            Some(user_id) => Ok(vec![user_id.value().to_string()]),
            None => Ok(vec![]), // Return empty vector if no record found
        }
    }

    fn apply_order<E>(&self, results: Vec<E>, column: &str, ascending: bool) -> Result<Vec<E>>
    where
        E: vantage_core::Entity,
    {
        // Serialize all entities to JSON for ordering
        let mut json_results: Vec<(serde_json::Value, E)> = Vec::new();

        for entity in results {
            let json_value = serde_json::to_value(&entity)
                .with_context(|| "Failed to serialize entity for ordering".to_string())?;
            json_results.push((json_value, entity));
        }

        // Check if any entity had the field, if not return error
        json_results
            .iter()
            .find(|(json, _)| json.get(column).is_some())
            .ok_or_else(|| vantage_error!("Field '{}' not found in any entity", column))?;

        // Sort by the specified column
        json_results.sort_by(|(json_a, _), (json_b, _)| {
            let val_a = json_a.get(column);
            let val_b = json_b.get(column);

            let cmp = match (val_a, val_b) {
                (Some(a), Some(b)) => {
                    if let (Some(a_str), Some(b_str)) = (a.as_str(), b.as_str()) {
                        a_str.cmp(b_str)
                    } else if let (Some(a_num), Some(b_num)) = (a.as_u64(), b.as_u64()) {
                        a_num.cmp(&b_num)
                    } else if let (Some(a_bool), Some(b_bool)) = (a.as_bool(), b.as_bool()) {
                        a_bool.cmp(&b_bool)
                    } else {
                        std::cmp::Ordering::Equal
                    }
                }
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (None, None) => std::cmp::Ordering::Equal,
            };

            if ascending { cmp } else { cmp.reverse() }
        });

        // Extract the sorted entities
        Ok(json_results.into_iter().map(|(_, entity)| entity).collect())
    }

    /// Execute a RedbSelect with entity type information
    pub async fn redb_execute_select<E>(&self, select: &crate::RedbSelect<E>) -> Result<Vec<E>>
    where
        E: vantage_core::Entity,
    {
        let table_name = select.table().map(|s| s.as_str()).unwrap_or("users");

        let records = if let Some(key_expr) = select.key() {
            let (column, value) = key_expr
                .as_eq()
                .ok_or_else(|| vantage_error!("ReDB only supports Eq conditions"))?;

            let mut ids = self.get_ids_by_condition(table_name, column, value).await?;

            // Apply skip first
            if let Some(skip) = select.skip() {
                let skip = skip as usize;
                if skip < ids.len() {
                    ids = ids[skip..].to_vec();
                } else {
                    ids = vec![];
                }
            }

            // Then apply limit
            if let Some(limit) = select.limit() {
                let limit = limit as usize;
                if ids.len() > limit {
                    ids = ids[..limit].to_vec();
                }
            }

            self.get_records_by_ids(table_name, ids).await?
        } else {
            self.get_all_records::<E>(
                table_name,
                select.limit().map(|l| l as usize),
                select.skip().unwrap_or(0) as usize,
            )
            .await?
        };

        if let Some(order_col) = select.order_column() {
            self.apply_order(records, order_col, select.order_ascending())
        } else {
            Ok(records)
        }
    }
}
