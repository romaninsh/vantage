use ciborium::Value as CborValue;
use sqlx::mysql::{MySqlQueryResult, MySqlRow};
use vantage_expressions::Expression;
use vantage_expressions::traits::expressive::DeferredFn;
use vantage_types::Record;

use crate::mysql::MysqlDB;
use crate::mysql::row::{bind_mysql_value, row_to_record};
use crate::mysql::types::AnyMysqlType;
use crate::sql_exec::{self, Placeholder, SqlDialect, SqlxQuery};

impl SqlDialect for MysqlDB {
    type Db = sqlx::MySql;
    type Value = AnyMysqlType;

    const PLACEHOLDER: Placeholder = Placeholder::Question;
    const QUERY_FAILED: &'static str = "MySQL query failed";
    const STATEMENT_FAILED: &'static str = "MySQL statement failed";

    fn sqlx_pool(&self) -> &sqlx::MySqlPool {
        self.pool()
    }

    fn bind<'q>(
        query: SqlxQuery<'q, sqlx::MySql>,
        value: &'q AnyMysqlType,
    ) -> vantage_core::Result<SqlxQuery<'q, sqlx::MySql>> {
        Ok(bind_mysql_value(query, value))
    }

    fn rows_affected(result: &MySqlQueryResult) -> u64 {
        result.rows_affected()
    }

    fn row_to_record(row: &MySqlRow) -> Record<AnyMysqlType> {
        row_to_record(row)
    }

    fn into_cbor(value: AnyMysqlType) -> CborValue {
        value.into_value()
    }

    fn from_cbor_rows(rows: CborValue) -> AnyMysqlType {
        AnyMysqlType::from_cbor(&rows).expect("CBOR array should always convert to AnyMysqlType")
    }

    fn untyped(value: CborValue) -> AnyMysqlType {
        AnyMysqlType::untyped(value)
    }
}

impl vantage_expressions::ExprDataSource<AnyMysqlType> for MysqlDB {
    async fn execute(&self, expr: &Expression<AnyMysqlType>) -> vantage_core::Result<AnyMysqlType> {
        sql_exec::fetch_rows(self, expr).await
    }

    fn defer(&self, expr: Expression<AnyMysqlType>) -> DeferredFn<AnyMysqlType> {
        sql_exec::defer_first_cell(self, expr)
    }
}

impl MysqlDB {
    /// Run a statement that returns no rows (DELETE, UPDATE) and report how
    /// many rows it changed. [`ExprDataSource::execute`](vantage_expressions::ExprDataSource::execute)
    /// returns the fetched rows instead, so it can't tell "deleted one" from
    /// "matched nothing".
    pub async fn execute_affected(
        &self,
        expr: &Expression<AnyMysqlType>,
    ) -> vantage_core::Result<u64> {
        sql_exec::execute_affected(self, expr).await
    }
}
