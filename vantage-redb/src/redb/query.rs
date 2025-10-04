//! Query execution methods for ReDB

use redb::{ReadableTable, TableDefinition};

use super::core::Redb;

impl Redb {
    pub async fn get_by_key<E>(
        &self,
        table_name: &str,
        key_expr: &crate::expression::RedbExpression,
    ) -> serde_json::Value
    where
        E: vantage_core::Entity,
    {
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table_name);
        let read_txn = match self.begin_read() {
            Ok(txn) => txn,
            Err(e) => return serde_json::json!({"error": format!("Transaction error: {}", e)}),
        };

        let table = match read_txn.open_table(table_def) {
            Ok(table) => table,
            Err(e) => return serde_json::json!({"error": format!("Table error: {}", e)}),
        };

        if let Some(key_str) = key_expr.value().as_str() {
            match table.get(key_str) {
                Ok(Some(data)) => {
                    match self.deserialize_to_json_with_id::<E>(data.value(), key_str) {
                        Ok(json) => json,
                        Err(e) => serde_json::json!({"error": format!("Deserialize error: {}", e)}),
                    }
                }
                Ok(None) => serde_json::json!(null),
                Err(e) => serde_json::json!({"error": format!("Key lookup error: {}", e)}),
            }
        } else {
            serde_json::json!({"error": "Invalid key format"})
        }
    }

    pub async fn get_by_condition<E>(
        &self,
        table_name: &str,
        column: &str,
        value: &serde_json::Value,
        limit: usize,
    ) -> serde_json::Value
    where
        E: vantage_core::Entity,
    {
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table_name);
        let read_txn = match self.begin_read() {
            Ok(txn) => txn,
            Err(e) => return serde_json::json!({"error": format!("Transaction error: {}", e)}),
        };

        let table = match read_txn.open_table(table_def) {
            Ok(table) => table,
            Err(e) => return serde_json::json!({"error": format!("Table error: {}", e)}),
        };

        let mut results = Vec::new();
        let mut count = 0;

        match table.iter() {
            Ok(iter) => {
                for item in iter {
                    if count >= limit {
                        break;
                    }

                    if let Ok((id, data)) = item
                        && let Ok(record) =
                            self.deserialize_to_json_with_id::<E>(data.value(), id.value())
                        && let Some(field_value) = record.get(column)
                        && field_value == value
                    {
                        results.push(record);
                        count += 1;
                    }
                }
                serde_json::Value::Array(results)
            }
            Err(e) => serde_json::json!({"error": format!("Iteration error: {}", e)}),
        }
    }

    pub async fn get_all_records<E>(
        &self,
        table_name: &str,
        limit: usize,
        skip: usize,
    ) -> serde_json::Value
    where
        E: vantage_core::Entity,
    {
        let table_def: TableDefinition<&str, &[u8]> = TableDefinition::new(table_name);
        let read_txn = match self.begin_read() {
            Ok(txn) => txn,
            Err(e) => return serde_json::json!({"error": format!("Transaction error: {}", e)}),
        };

        let table = match read_txn.open_table(table_def) {
            Ok(table) => table,
            Err(e) => return serde_json::json!({"error": format!("Table error: {}", e)}),
        };

        let mut results = Vec::new();
        let mut count = 0;

        match table.iter() {
            Ok(iter) => {
                for (i, item) in iter.enumerate() {
                    if i < skip {
                        continue;
                    }
                    if count >= limit {
                        break;
                    }

                    if let Ok((id, data)) = item
                        && let Ok(record) =
                            self.deserialize_to_json_with_id::<E>(data.value(), id.value())
                    {
                        results.push(record);
                        count += 1;
                    }
                }
                serde_json::Value::Array(results)
            }
            Err(e) => serde_json::json!({"error": format!("Iteration error: {}", e)}),
        }
    }

    pub fn order_results(&self, results: &mut serde_json::Value, column: &str, ascending: bool) {
        if let serde_json::Value::Array(records) = results {
            records.sort_by(|a, b| {
                let val_a = a.get(column);
                let val_b = b.get(column);

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
        }
    }

    pub fn apply_limit(
        &self,
        results: &mut serde_json::Value,
        limit: Option<i64>,
        skip: Option<i64>,
    ) {
        if let serde_json::Value::Array(records) = results {
            let skip_count = skip.unwrap_or(0) as usize;
            let limit_count = limit.map(|l| l as usize);

            if skip_count > 0 && skip_count < records.len() {
                records.drain(0..skip_count);
            } else if skip_count >= records.len() {
                records.clear();
                return;
            }

            if let Some(limit_val) = limit_count
                && records.len() > limit_val
            {
                records.truncate(limit_val);
            }
        }
    }

    /// Generic deserialization method that works with any entity type.
    fn deserialize_to_json_with_id<E>(
        &self,
        data: &[u8],
        id: &str,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>
    where
        E: vantage_core::Entity,
    {
        match bincode::deserialize::<E>(data) {
            Ok(entity) => {
                let mut json_value = serde_json::to_value(entity)?;
                if let serde_json::Value::Object(ref mut map) = json_value {
                    map.insert("id".to_string(), serde_json::Value::String(id.to_string()));
                }
                Ok(json_value)
            }
            Err(e) => Err(Box::new(e)),
        }
    }

    /// Execute a RedbSelect with entity type information
    pub async fn execute_select<E>(&self, select: &crate::RedbSelect<E>) -> serde_json::Value
    where
        E: vantage_core::Entity,
    {
        let table_name = select.table().map(|s| s.as_str()).unwrap_or("users");

        if let Some(key_expr) = select.key() {
            return self.get_by_key::<E>(table_name, key_expr).await;
        }

        let mut results = if let (Some(column), Some(value)) =
            (select.condition_column(), select.condition_value())
        {
            self.get_by_condition::<E>(
                table_name,
                column,
                value,
                select.limit().unwrap_or(1000) as usize,
            )
            .await
        } else {
            self.get_all_records::<E>(
                table_name,
                select.limit().unwrap_or(1000) as usize,
                select.skip().unwrap_or(0) as usize,
            )
            .await
        };

        if let Some(order_col) = select.order_column() {
            self.order_results(&mut results, order_col, select.order_ascending());
        }

        // Apply limit and skip for ordered results or condition-based queries
        if select.order_column().is_some() || select.condition_column().is_some() {
            self.apply_limit(&mut results, select.limit(), select.skip());
        }

        results
    }
}
