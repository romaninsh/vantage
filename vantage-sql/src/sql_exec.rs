//! Statement plumbing shared by the SQLite, PostgreSQL and MySQL data
//! sources: resolve deferred parameters, render `{}` placeholders in the
//! dialect's style, bind, run, and turn rows into the dialect's value type.

#[cfg(any(feature = "postgres", feature = "mysql"))]
mod defer;

#[cfg(any(feature = "postgres", feature = "mysql"))]
pub(crate) use defer::{defer_first_cell, first_cell};

use ciborium::Value as CborValue;
use sqlx::{AssertSqlSafe, Database, Executor, IntoArguments, Pool};
use vantage_core::{Context, Result, VantageError, error};
use vantage_expressions::{Expression, ExpressionFlattener, ExpressiveEnum, Flatten};
use vantage_types::Record;

use crate::sql_tx::SqlTx;

pub(crate) type SqlxQuery<'q, DB> = sqlx::query::Query<'q, DB, <DB as Database>::Arguments>;

/// How a dialect spells the n-th positional parameter.
#[derive(Clone, Copy)]
// A variant is built only when its dialect's feature is on.
#[cfg_attr(
    not(all(feature = "sqlite", feature = "postgres", feature = "mysql")),
    allow(dead_code)
)]
pub(crate) enum Placeholder {
    /// `?1`, `?2`, … (SQLite)
    QuestionNumbered,
    /// `$1`, `$2`, … (PostgreSQL)
    DollarNumbered,
    /// `?` (MySQL)
    Question,
}

impl Placeholder {
    fn push(self, sql: &mut String, n: usize) {
        match self {
            Placeholder::QuestionNumbered => sql.push_str(&format!("?{n}")),
            Placeholder::DollarNumbered => sql.push_str(&format!("${n}")),
            Placeholder::Question => sql.push('?'),
        }
    }
}

/// What differs between the sqlx-backed dialects.
pub(crate) trait SqlDialect {
    type Db: Database;
    type Value: Clone + Send + Sync + 'static;

    const PLACEHOLDER: Placeholder;
    /// Error message for a failed row-returning query.
    const QUERY_FAILED: &'static str;
    /// Error message for a failed [`execute_affected`] statement.
    const STATEMENT_FAILED: &'static str;

    fn sqlx_pool(&self) -> &Pool<Self::Db>;

    /// The transaction this datasource runs its statements on, when it is in
    /// transaction mode.
    fn sql_tx(&self) -> Option<&SqlTx<Self::Db>>;

    /// Bind one parameter. Errors when the value can't be expressed as a
    /// parameter of this dialect.
    fn bind<'q>(
        query: SqlxQuery<'q, Self::Db>,
        value: &'q Self::Value,
    ) -> Result<SqlxQuery<'q, Self::Db>>;

    fn rows_affected(result: &<Self::Db as Database>::QueryResult) -> u64;

    fn row_to_record(row: &<Self::Db as Database>::Row) -> Record<Self::Value>;

    fn into_cbor(value: Self::Value) -> CborValue;

    /// Wrap the CBOR array of row maps as the dialect's value.
    fn from_cbor_rows(rows: CborValue) -> Self::Value;

    /// One cell's value, without a type tag.
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    fn untyped(value: CborValue) -> Self::Value;

    /// Parameter-type summary for error context (types only, never values);
    /// `None` leaves it out.
    fn describe_params(_params: &[Self::Value]) -> Option<String> {
        None
    }
}

/// Run a row-returning query; each row becomes a CBOR map in an array.
pub(crate) async fn fetch_rows<D>(db: &D, expr: &Expression<D::Value>) -> Result<D::Value>
where
    D: SqlDialect,
    for<'c> &'c mut <D::Db as Database>::Connection: Executor<'c, Database = D::Db>,
    <D::Db as Database>::Arguments: IntoArguments<D::Db>,
{
    // Deferred parameters may query this same datasource, so they resolve
    // before the transaction's connection is taken.
    let (sql, params) = prepare(expr, D::PLACEHOLDER).await?;
    match db.sql_tx() {
        None => fetch_prepared::<D, _>(db.sqlx_pool(), &sql, &params).await,
        Some(tx) => {
            let mut open = tx.open().await?;
            tx.watch(fetch_prepared::<D, _>(&mut **open, &sql, &params))
                .await
        }
    }
}

/// [`fetch_rows`] on a given executor — a connection or an open transaction.
#[cfg(all(feature = "mysql", feature = "vista"))]
pub(crate) async fn fetch_rows_on<'c, D, X>(
    executor: X,
    expr: &Expression<D::Value>,
) -> Result<D::Value>
where
    D: SqlDialect,
    X: Executor<'c, Database = D::Db>,
    <D::Db as Database>::Arguments: IntoArguments<D::Db>,
{
    let (sql, params) = prepare(expr, D::PLACEHOLDER).await?;
    fetch_prepared::<D, _>(executor, &sql, &params).await
}

async fn fetch_prepared<'c, D, X>(executor: X, sql: &str, params: &[D::Value]) -> Result<D::Value>
where
    D: SqlDialect,
    X: Executor<'c, Database = D::Db>,
    <D::Db as Database>::Arguments: IntoArguments<D::Db>,
{
    let rows = bind_all::<D>(sql, params)?
        .fetch_all(executor)
        .await
        .map_err(|e| {
            let err = refusal(&e).unwrap_or_else(|| {
                mark_transport(error!(D::QUERY_FAILED, details = e.to_string()), &e)
            });
            failure_context::<D>(err, sql, params)
        })?;

    let rows = rows
        .iter()
        .map(|row| {
            CborValue::Map(
                D::row_to_record(row)
                    .into_iter()
                    .map(|(k, v)| (CborValue::Text(k), D::into_cbor(v)))
                    .collect(),
            )
        })
        .collect();
    Ok(D::from_cbor_rows(CborValue::Array(rows)))
}

/// Run a statement that returns no rows (DELETE, UPDATE) and report how many
/// rows it changed.
pub(crate) async fn execute_affected<D>(db: &D, expr: &Expression<D::Value>) -> Result<u64>
where
    D: SqlDialect,
    for<'c> &'c mut <D::Db as Database>::Connection: Executor<'c, Database = D::Db>,
    <D::Db as Database>::Arguments: IntoArguments<D::Db>,
{
    let (sql, params) = prepare(expr, D::PLACEHOLDER).await?;
    match db.sql_tx() {
        None => execute_prepared::<D, _>(db.sqlx_pool(), &sql, &params).await,
        Some(tx) => {
            let mut open = tx.open().await?;
            tx.watch(execute_prepared::<D, _>(&mut **open, &sql, &params))
                .await
        }
    }
}

/// [`execute_affected`] on a given executor — a connection or an open
/// transaction.
#[cfg(all(feature = "mysql", feature = "vista"))]
pub(crate) async fn execute_affected_on<'c, D, X>(
    executor: X,
    expr: &Expression<D::Value>,
) -> Result<u64>
where
    D: SqlDialect,
    X: Executor<'c, Database = D::Db>,
    <D::Db as Database>::Arguments: IntoArguments<D::Db>,
{
    let (sql, params) = prepare(expr, D::PLACEHOLDER).await?;
    execute_prepared::<D, _>(executor, &sql, &params).await
}

async fn execute_prepared<'c, D, X>(executor: X, sql: &str, params: &[D::Value]) -> Result<u64>
where
    D: SqlDialect,
    X: Executor<'c, Database = D::Db>,
    <D::Db as Database>::Arguments: IntoArguments<D::Db>,
{
    let done = match bind_all::<D>(sql, params)?.execute(executor).await {
        Ok(done) => done,
        Err(e) => match refusal(&e) {
            Some(err) => return Err(failure_context::<D>(err, sql, params)),
            None => {
                let err = mark_transport(error!(D::STATEMENT_FAILED), &e);
                Err(e).with_context(|| failure_context::<D>(err, sql, params))?
            }
        },
    };
    Ok(D::rows_affected(&done))
}

/// A trigger or procedure refusing the write on purpose: SQLite's
/// `RAISE(ABORT, …)` (constraint-trigger code 1811), PostgreSQL's `RAISE
/// EXCEPTION` (P0001), MySQL's `SIGNAL SQLSTATE '45000'`. Its message is
/// meant for the user, so it becomes the error's message. `refused_by` is
/// the first context key (the server looks for ` (refused_by: `).
fn refusal(e: &sqlx::Error) -> Option<VantageError> {
    let db = e.as_database_error()?;
    let code = db.code()?;
    matches!(code.as_ref(), "1811" | "P0001" | "45000")
        .then(|| error!(db.message().to_string(), refused_by = "database").mark_invalid_value())
}

/// The context key a failed statement's error carries when the database
/// refused it for a duplicate unique or primary key.
pub(crate) const VIOLATION: &str = "violation";
pub(crate) const UNIQUE_VIOLATION: &str = "unique";

/// Context key [`mark_transport`] sets when the statement never reached the
/// database (the pool gave no connection).
pub(crate) const NOT_SENT: &str = "not_sent";

/// Whether `err` is a duplicate unique or primary key ([`mark_transport`]).
pub(crate) fn is_unique_violation(err: &VantageError) -> bool {
    err.context.get(VIOLATION).map(String::as_str) == Some(UNIQUE_VIOLATION)
}

/// Whether `err`'s statement never reached the database ([`mark_transport`]):
/// nothing it would have written can be there.
pub(crate) fn was_not_sent(err: &VantageError) -> bool {
    err.context.contains_key(NOT_SENT)
}

/// `err` marked Unavailable when `e` says the database couldn't be reached
/// or the pool had no connection to give: a retry may land. Errors the
/// database itself returned (constraints, bad values) stay as they are.
pub(crate) fn mark_transport(mut err: VantageError, e: &sqlx::Error) -> VantageError {
    if e.as_database_error()
        .is_some_and(|db| db.is_unique_violation())
    {
        err.context
            .insert(VIOLATION.to_string(), UNIQUE_VIOLATION.to_string());
    }
    if matches!(e, sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed) {
        err.context.insert(NOT_SENT.to_string(), "true".to_string());
    }
    match e {
        sqlx::Error::Io(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => err.mark_unavailable(),
        _ => err,
    }
}

/// Resolve deferred parameters (which may query other databases), then
/// flatten and render placeholders.
pub(crate) async fn prepare<V: Clone>(
    expr: &Expression<V>,
    placeholder: Placeholder,
) -> Result<(String, Vec<V>)> {
    let resolved = expr.resolve_deferred().await?;
    prepare_typed_query(&resolved, placeholder)
}

/// Bind every parameter onto `sql`. A bind error names the parameter and
/// carries the SQL.
pub(crate) fn bind_all<'q, D: SqlDialect>(
    sql: &'q str,
    params: &'q [D::Value],
) -> Result<SqlxQuery<'q, D::Db>> {
    // Values travel as bound parameters, so the rendered text is safe to run.
    let mut query = sqlx::query(AssertSqlSafe(sql));
    for (i, value) in params.iter().enumerate() {
        query = D::bind(query, value).map_err(|mut e| {
            e.context.insert("parameter".into(), (i + 1).to_string());
            e.context.insert("sql".into(), truncate_sql(sql));
            e
        })?;
    }
    Ok(query)
}

/// Add the statement (and the dialect's parameter summary) to `err`.
pub(crate) fn failure_context<D: SqlDialect>(
    mut err: VantageError,
    sql: &str,
    params: &[D::Value],
) -> VantageError {
    err.context
        .insert("sql".into(), format!("{:?}", truncate_sql(sql)));
    if let Some(types) = D::describe_params(params) {
        err.context.insert("params".into(), format!("{types:?}"));
    }
    err
}

/// SQL for error context — whole statement up to a cap, so a giant
/// generated query can't balloon an error message.
fn truncate_sql(sql: &str) -> String {
    const MAX: usize = 500;
    if sql.len() <= MAX {
        sql.to_string()
    } else {
        let cut: String = sql.chars().take(MAX).collect();
        format!("{cut}…")
    }
}

/// Flatten an expression and render its `{}` placeholders in `placeholder`
/// style. Expects deferred parameters to be resolved already.
fn prepare_typed_query<V: Clone>(
    expr: &Expression<V>,
    placeholder: Placeholder,
) -> Result<(String, Vec<V>)> {
    let flattened = ExpressionFlattener::new().flatten(expr);
    let template_parts: Vec<&str> = flattened.template.split("{}").collect();

    if template_parts.len() != flattened.parameters.len() + 1 {
        return Err(error!(
            "template placeholder count doesn't match parameter count",
            placeholders = template_parts.len() - 1,
            parameters = flattened.parameters.len()
        ));
    }

    let mut sql = String::from(template_parts[0]);
    let mut params = Vec::new();
    for (i, param) in flattened.parameters.iter().enumerate() {
        match param {
            ExpressiveEnum::Scalar(value) => {
                params.push(value.clone());
                placeholder.push(&mut sql, params.len());
            }
            ExpressiveEnum::Nested(_) => {
                unreachable!(
                    "nested expression should have been flattened during query preparation"
                );
            }
            ExpressiveEnum::Deferred(_) => {
                unreachable!("deferred expression should have been resolved before prepare");
            }
        }
        sql.push_str(template_parts[i + 1]);
    }

    Ok((sql, params))
}
