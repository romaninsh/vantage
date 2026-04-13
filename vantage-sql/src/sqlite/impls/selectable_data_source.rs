use vantage_expressions::traits::datasource::SelectableDataSource;
use vantage_expressions::{Expression, Expressive, Selectable};

use crate::primitives::alias::AliasExt;
use crate::sqlite::SqliteDB;
use crate::sqlite::statements::SqliteSelect;
use crate::sqlite::types::AnySqliteType;

impl SelectableDataSource<AnySqliteType, crate::condition::SqliteCondition> for SqliteDB {
    type Select = SqliteSelect;

    fn select(&self) -> Self::Select {
        SqliteSelect::new()
    }

    fn add_select_column(
        &self,
        select: &mut Self::Select,
        expression: Expression<AnySqliteType>,
        alias: Option<&str>,
    ) {
        match alias {
            Some(a) => select.add_expression(expression.as_alias(a)),
            None => select.add_expression(expression),
        }
    }

    async fn execute_select(
        &self,
        select: &Self::Select,
    ) -> vantage_core::Result<Vec<AnySqliteType>> {
        use vantage_expressions::ExprDataSource;

        let result = self.execute(&select.expr()).await?;

        // Result is an array of row objects — unwrap into individual AnySqliteType values
        match result.value() {
            ciborium::Value::Array(arr) => Ok(arr
                .iter()
                .map(|v| AnySqliteType::untyped(v.clone()))
                .collect()),
            _ => Ok(vec![result]),
        }
    }
}
