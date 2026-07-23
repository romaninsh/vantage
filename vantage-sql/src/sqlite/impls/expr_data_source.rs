use ciborium::Value as CborValue;
use vantage_expressions::traits::expressive::DeferredFn;
use vantage_expressions::{Expression, ExpressionFlattener, ExpressiveEnum, Flatten};

use crate::sqlite::SqliteDB;
use crate::sqlite::row::{bind_sqlite_value, describe_param_types, row_to_record};
use crate::sqlite::types::AnySqliteType;

impl vantage_expressions::ExprDataSource<AnySqliteType> for SqliteDB {
    async fn execute(
        &self,
        expr: &Expression<AnySqliteType>,
    ) -> vantage_core::Result<AnySqliteType> {
        // 1. Resolve deferred parameters (async — may call other databases)
        let resolved = resolve_deferred(expr).await?;

        // 2. Flatten nested expressions + convert {} to ?N
        let (sql, params) = prepare_typed_query(&resolved)?;

        // 3. Bind and execute. Errors carry the SQL and a parameter-type
        // summary (types only, never values) — the SQL names the table
        // and columns, which is what failure reports need most.
        let mut query = sqlx::query(&sql);
        for (i, value) in params.iter().enumerate() {
            query = bind_sqlite_value(query, value).map_err(|mut e| {
                e.context.insert("parameter".into(), (i + 1).to_string());
                e.context.insert("sql".into(), truncate_sql(&sql));
                e
            })?;
        }

        let rows = query.fetch_all(self.pool()).await.map_err(|e| {
            vantage_core::error!(
                "SQLite query failed",
                details = e.to_string(),
                sql = truncate_sql(&sql),
                params = describe_param_types(&params)
            )
        })?;

        // 4. Convert rows to AnySqliteType — each row becomes a CBOR Map
        let arr: Vec<CborValue> = rows
            .iter()
            .map(|row| {
                let record = row_to_record(row);
                let map: Vec<(CborValue, CborValue)> = record
                    .into_iter()
                    .map(|(k, v)| (CborValue::Text(k), v.into_value()))
                    .collect();
                CborValue::Map(map)
            })
            .collect();

        let cbor_arr = CborValue::Array(arr);
        Ok(AnySqliteType::from_cbor(&cbor_arr)
            .expect("CBOR array should always convert to AnySqliteType"))
    }

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

/// Resolve all Deferred parameters in an expression by calling them.
/// Deferred closures may execute queries on other databases.
async fn resolve_deferred(
    expr: &Expression<AnySqliteType>,
) -> vantage_core::Result<Expression<AnySqliteType>> {
    let mut resolved_params = Vec::new();

    for param in &expr.parameters {
        match param {
            ExpressiveEnum::Deferred(deferred_fn) => {
                let result = deferred_fn.call().await?;
                resolved_params.push(result);
            }
            ExpressiveEnum::Nested(inner) => {
                let resolved_inner = Box::pin(resolve_deferred(inner)).await?;
                resolved_params.push(ExpressiveEnum::Nested(resolved_inner));
            }
            other => {
                resolved_params.push(other.clone());
            }
        }
    }

    Ok(Expression::new(expr.template.clone(), resolved_params))
}

/// Flatten an `Expression<AnySqliteType>` and convert `{}` placeholders to `?N`.
fn prepare_typed_query(
    expr: &Expression<AnySqliteType>,
) -> vantage_core::Result<(String, Vec<AnySqliteType>)> {
    let flattener = ExpressionFlattener::new();
    let flattened = flattener.flatten(expr);

    let mut sql = String::new();
    let mut params = Vec::new();
    let template_parts: Vec<&str> = flattened.template.split("{}").collect();
    let mut param_counter = 0;

    if template_parts.len() != flattened.parameters.len() + 1 {
        return Err(vantage_core::error!(
            "template placeholder count doesn't match parameter count",
            placeholders = template_parts.len() - 1,
            parameters = flattened.parameters.len()
        ));
    }

    sql.push_str(template_parts[0]);

    for (i, param) in flattened.parameters.iter().enumerate() {
        match param {
            ExpressiveEnum::Scalar(value) => {
                param_counter += 1;
                sql.push_str(&format!("?{}", param_counter));
                params.push(value.clone());
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

        if i + 1 < template_parts.len() {
            sql.push_str(template_parts[i + 1]);
        }
    }

    Ok((sql, params))
}
