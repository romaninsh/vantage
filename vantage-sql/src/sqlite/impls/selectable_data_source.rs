use vantage_expressions::Expressive;
use vantage_expressions::traits::datasource::SelectableDataSource;

use crate::sqlite::SqliteDB;
use crate::sqlite::statements::SqliteSelect;
use crate::sqlite::types::AnySqliteType;

impl SelectableDataSource<AnySqliteType> for SqliteDB {
    type Select = SqliteSelect;

    fn select(&self) -> Self::Select {
        SqliteSelect::new()
    }

    async fn execute_select(&self, select: &Self::Select) -> vantage_core::Result<Vec<AnySqliteType>> {
        use vantage_expressions::ExprDataSource;

        let result = self.execute(&select.expr()).await?;

        // Result is an array of row objects — unwrap into individual AnySqliteType values
        match result.value() {
            serde_json::Value::Array(arr) => {
                Ok(arr.iter()
                    .map(|v| AnySqliteType::untyped(v.clone()))
                    .collect())
            }
            _ => Ok(vec![result]),
        }
    }
}
