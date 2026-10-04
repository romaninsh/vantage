use ciborium::Value as CborValue;
use sqlx::postgres::{PgQueryResult, PgRow};
use vantage_expressions::Expression;
use vantage_expressions::traits::expressive::DeferredFn;
use vantage_types::Record;

use crate::postgres::PostgresDB;
use crate::postgres::row::{bind_postgres_value, row_to_record};
use crate::postgres::types::AnyPostgresType;
use crate::sql_exec::{self, Placeholder, SqlDialect, SqlxQuery};

impl SqlDialect for PostgresDB {
    type Db = sqlx::Postgres;
    type Value = AnyPostgresType;

    const PLACEHOLDER: Placeholder = Placeholder::DollarNumbered;
    const QUERY_FAILED: &'static str = "PostgreSQL query failed";
    const STATEMENT_FAILED: &'static str = "PostgreSQL statement failed";

    fn sqlx_pool(&self) -> &sqlx::PgPool {
        self.pool()
    }

    fn bind<'q>(
        query: SqlxQuery<'q, sqlx::Postgres>,
        value: &'q AnyPostgresType,
    ) -> vantage_core::Result<SqlxQuery<'q, sqlx::Postgres>> {
        Ok(bind_postgres_value(query, value))
    }

    fn rows_affected(result: &PgQueryResult) -> u64 {
        result.rows_affected()
    }

    fn row_to_record(row: &PgRow) -> Record<AnyPostgresType> {
        row_to_record(row)
    }

    fn into_cbor(value: AnyPostgresType) -> CborValue {
        value.into_value()
    }

    fn from_cbor_rows(rows: CborValue) -> AnyPostgresType {
        AnyPostgresType::from_cbor(&rows)
            .expect("CBOR array should always convert to AnyPostgresType")
    }

    fn untyped(value: CborValue) -> AnyPostgresType {
        AnyPostgresType::untyped(value)
    }
}

impl vantage_expressions::ExprDataSource<AnyPostgresType> for PostgresDB {
    async fn execute(
        &self,
        expr: &Expression<AnyPostgresType>,
    ) -> vantage_core::Result<AnyPostgresType> {
        sql_exec::fetch_rows(self, expr).await
    }

    fn defer(&self, expr: Expression<AnyPostgresType>) -> DeferredFn<AnyPostgresType> {
        sql_exec::defer_first_cell(self, expr)
    }
}

impl PostgresDB {
    /// Run a statement that returns no rows (DELETE, UPDATE) and report how
    /// many rows it changed. [`ExprDataSource::execute`](vantage_expressions::ExprDataSource::execute)
    /// returns the fetched rows instead, so it can't tell "deleted one" from
    /// "matched nothing".
    pub async fn execute_affected(
        &self,
        expr: &Expression<AnyPostgresType>,
    ) -> vantage_core::Result<u64> {
        sql_exec::execute_affected(self, expr).await
    }
}
