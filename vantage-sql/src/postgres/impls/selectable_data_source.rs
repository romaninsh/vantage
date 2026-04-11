use vantage_expressions::Expressive;
use vantage_expressions::traits::datasource::SelectableDataSource;

use crate::postgres::PostgresDB;
use crate::postgres::statements::PostgresSelect;
use crate::postgres::types::AnyPostgresType;

impl SelectableDataSource<AnyPostgresType> for PostgresDB {
    type Select = PostgresSelect;

    fn select(&self) -> Self::Select {
        PostgresSelect::new()
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
