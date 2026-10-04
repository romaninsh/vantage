use ciborium::Value as CborValue;
use sqlx::sqlite::{SqliteQueryResult, SqliteRow};
use vantage_expressions::Expression;
use vantage_expressions::traits::expressive::DeferredFn;
use vantage_types::Record;

use crate::sql_exec::{self, Placeholder, SqlDialect, SqlxQuery};
use crate::sqlite::SqliteDB;
use crate::sqlite::row::{bind_sqlite_value, describe_param_types, row_to_record};
use crate::sqlite::types::AnySqliteType;

impl SqlDialect for SqliteDB {
    type Db = sqlx::Sqlite;
    type Value = AnySqliteType;

    const PLACEHOLDER: Placeholder = Placeholder::QuestionNumbered;
    const QUERY_FAILED: &'static str = "SQLite query failed";
    const STATEMENT_FAILED: &'static str = "SQLite statement failed";

    fn sqlx_pool(&self) -> &sqlx::SqlitePool {
        self.pool()
    }

    fn bind<'q>(
        query: SqlxQuery<'q, sqlx::Sqlite>,
        value: &'q AnySqliteType,
    ) -> vantage_core::Result<SqlxQuery<'q, sqlx::Sqlite>> {
        bind_sqlite_value(query, value)
    }

    fn rows_affected(result: &SqliteQueryResult) -> u64 {
        result.rows_affected()
    }

    fn row_to_record(row: &SqliteRow) -> Record<AnySqliteType> {
        row_to_record(row)
    }

    fn into_cbor(value: AnySqliteType) -> CborValue {
        value.into_value()
    }

    fn from_cbor_rows(rows: CborValue) -> AnySqliteType {
        AnySqliteType::from_cbor(&rows).expect("CBOR array should always convert to AnySqliteType")
    }

    #[cfg(any(feature = "postgres", feature = "mysql"))]
    fn untyped(value: CborValue) -> AnySqliteType {
        AnySqliteType::untyped(value)
    }

    // The SQL names the table and columns, which is what failure reports
    // need most; the parameter summary adds types, never values.
    fn describe_params(params: &[AnySqliteType]) -> Option<String> {
        Some(describe_param_types(params))
    }
}

impl vantage_expressions::ExprDataSource<AnySqliteType> for SqliteDB {
    async fn execute(
        &self,
        expr: &Expression<AnySqliteType>,
    ) -> vantage_core::Result<AnySqliteType> {
        sql_exec::fetch_rows(self, expr).await
    }

    /// Unlike PostgreSQL and MySQL (`sql_exec::defer_first_cell`), only a
    /// one-row, one-column answer collapses to its cell; anything wider stays
    /// the full row array, so a deferred list (e.g. an `IN (…)` source) keeps
    /// every row. Moving SQLite onto the first-cell rule would change what
    /// its existing deferred queries resolve to.
    fn defer(&self, expr: Expression<AnySqliteType>) -> DeferredFn<AnySqliteType> {
        let db = self.clone();
        DeferredFn::from_fn(move || {
            let db = db.clone();
            let expr = expr.clone();
            Box::pin(async move {
                let result = vantage_expressions::ExprDataSource::execute(&db, &expr).await?;
                Ok(result.unwrap_scalar())
            })
        })
    }
}

impl SqliteDB {
    /// Run a statement that returns no rows (DELETE, UPDATE) and report how
    /// many rows it changed. [`ExprDataSource::execute`](vantage_expressions::ExprDataSource::execute)
    /// returns the fetched rows instead, so it can't tell "deleted one" from
    /// "matched nothing".
    pub async fn execute_affected(
        &self,
        expr: &Expression<AnySqliteType>,
    ) -> vantage_core::Result<u64> {
        sql_exec::execute_affected(self, expr).await
    }
}
