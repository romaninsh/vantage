//! Statement plumbing shared by the SQLite, PostgreSQL and MySQL data
//! sources: resolve deferred parameters, render `{}` placeholders in the
//! dialect's style, bind, run, and turn rows into the dialect's value type.

#[cfg(any(feature = "postgres", feature = "mysql"))]
mod defer;

#[cfg(any(feature = "postgres", feature = "mysql"))]
pub(crate) use defer::{defer_first_cell, first_cell};

use ciborium::Value as CborValue;
use sqlx::{Database, Executor, IntoArguments, Pool};
use vantage_core::{Context, Result, VantageError, error};
use vantage_expressions::{Expression, ExpressionFlattener, ExpressiveEnum, Flatten};
use vantage_types::Record;

pub(crate) type SqlxQuery<'q, DB> = sqlx::query::Query<'q, DB, <DB as Database>::Arguments<'q>>;

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
    for<'q> <D::Db as Database>::Arguments<'q>: IntoArguments<'q, D::Db>,
{
    let (sql, params) = prepare(expr, D::PLACEHOLDER).await?;
    let rows = bind_all::<D>(&sql, &params)?
        .fetch_all(db.sqlx_pool())
        .await
        .map_err(|e| {
            let err = error!(D::QUERY_FAILED, details = e.to_string());
            failure_context::<D>(err, &sql, &params)
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
    for<'q> <D::Db as Database>::Arguments<'q>: IntoArguments<'q, D::Db>,
{
    let (sql, params) = prepare(expr, D::PLACEHOLDER).await?;
    let done = bind_all::<D>(&sql, &params)?
        .execute(db.sqlx_pool())
        .await
        .with_context(|| failure_context::<D>(error!(D::STATEMENT_FAILED), &sql, &params))?;
    Ok(D::rows_affected(&done))
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
    let mut query = sqlx::query(sql);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn expr(template: &str, n: usize) -> Expression<i64> {
        Expression::new(
            template,
            (0..n as i64).map(ExpressiveEnum::Scalar).collect(),
        )
    }

    #[test]
    fn renders_each_placeholder_style() {
        let e = expr("a = {} AND b = {}", 2);
        let render = |p| prepare_typed_query(&e, p).unwrap().0;
        assert_eq!(render(Placeholder::QuestionNumbered), "a = ?1 AND b = ?2");
        assert_eq!(render(Placeholder::DollarNumbered), "a = $1 AND b = $2");
        assert_eq!(render(Placeholder::Question), "a = ? AND b = ?");
    }

    #[test]
    fn truncates_long_sql() {
        let long = "x".repeat(600);
        assert_eq!(truncate_sql(&long).chars().count(), 501);
        assert_eq!(truncate_sql("short"), "short");
    }
}
