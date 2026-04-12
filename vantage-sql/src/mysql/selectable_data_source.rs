use vantage_expressions::traits::datasource::SelectableDataSource;
use vantage_expressions::{Expression, Expressive, Selectable};

use crate::mysql::MysqlDB;
use crate::mysql::statements::MysqlSelect;
use crate::mysql::types::AnyMysqlType;
use crate::primitives::alias::AliasExt;

impl SelectableDataSource<AnyMysqlType> for MysqlDB {
    type Select = MysqlSelect;

    fn select(&self) -> Self::Select {
        MysqlSelect::new()
    }

    fn add_select_column(
        &self,
        select: &mut Self::Select,
        expression: Expression<AnyMysqlType>,
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
