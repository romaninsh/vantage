use vantage_expressions::traits::datasource::SelectableDataSource;
use vantage_expressions::{Expression, Expressive, Selectable};

use crate::postgres::PostgresDB;
use crate::postgres::statements::PostgresSelect;
use crate::postgres::types::AnyPostgresType;
use crate::primitives::alias::AliasExt;

impl SelectableDataSource<AnyPostgresType, crate::condition::PostgresCondition> for PostgresDB {
    type Select = PostgresSelect;

    fn select(&self) -> Self::Select {
        PostgresSelect::new()
    }

    fn add_select_column(
        &self,
        select: &mut Self::Select,
        expression: Expression<AnyPostgresType>,
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
    ) -> vantage_core::Result<Vec<AnyPostgresType>> {
        use vantage_expressions::ExprDataSource;

        let result = self.execute(&select.expr()).await?;

        match result.value() {
            ciborium::Value::Array(arr) => Ok(arr
                .iter()
                .map(|v| AnyPostgresType::untyped(v.clone()))
                .collect()),
            _ => Ok(vec![result]),
        }
    }
}
