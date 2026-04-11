use vantage_expressions::Expressive;
use vantage_expressions::traits::datasource::SelectableDataSource;

use crate::mysql::MysqlDB;
use crate::mysql::statements::MysqlSelect;
use crate::mysql::types::AnyMysqlType;

impl SelectableDataSource<AnyMysqlType> for MysqlDB {
    type Select = MysqlSelect;

    fn select(&self) -> Self::Select {
        MysqlSelect::new()
    }

    async fn execute_select(
        &self,
        select: &Self::Select,
    ) -> vantage_core::Result<Vec<AnyMysqlType>> {
        use vantage_expressions::ExprDataSource;

        let result = self.execute(&select.expr()).await?;

        match result.value() {
            ciborium::Value::Array(arr) => Ok(arr
                .iter()
                .map(|v| AnyMysqlType::untyped(v.clone()))
                .collect()),
            _ => Ok(vec![result]),
        }
    }
}
