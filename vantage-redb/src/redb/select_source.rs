//! SelectSource implementation for ReDB

use vantage_expressions::protocol::datasource::SelectSource;

use super::core::Redb;

// Note: SelectSource now supports generic Entity type
impl SelectSource<crate::expression::RedbExpression> for Redb {
    type Select<E>
        = crate::RedbSelect<E>
    where
        E: vantage_core::Entity;

    fn select<E>(&self) -> Self::Select<E>
    where
        E: vantage_core::Entity,
    {
        crate::RedbSelect::new()
    }

    async fn execute_select<E>(&self, select: &Self::Select<E>) -> serde_json::Value
    where
        E: vantage_core::Entity,
    {
        // Call the query implementation method from query.rs with Entity type
        let table_name = select.table().map(|s| s.as_str()).unwrap_or("users");

        if let Some(key_expr) = select.key() {
            // Key should always be an Eq condition for ReDB
            if let Some((column, value)) = key_expr.as_eq() {
                let mut results = self
                    .get_by_condition::<E>(
                        table_name,
                        column,
                        value,
                        select.limit().unwrap_or(1000) as usize,
                    )
                    .await;

                if let Some(order_col) = select.order_column() {
                    self.order_results(&mut results, order_col, select.order_ascending());
                }

                return results;
            } else {
                return serde_json::json!({"error": "ReDB only supports Eq conditions"});
            }
        }

        let mut results = self
            .get_all_records::<E>(
                table_name,
                select.limit().unwrap_or(1000) as usize,
                select.skip().unwrap_or(0) as usize,
            )
            .await;

        if let Some(order_col) = select.order_column() {
            self.order_results(&mut results, order_col, select.order_ascending());
        }

        // Apply limit and skip for ordered results
        if select.order_column().is_some() {
            self.apply_limit(&mut results, select.limit(), select.skip());
        }

        results
    }
}
