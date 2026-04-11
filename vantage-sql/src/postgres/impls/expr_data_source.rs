use ciborium::Value as CborValue;
use vantage_expressions::traits::expressive::DeferredFn;
use vantage_expressions::{Expression, ExpressionFlattener, ExpressiveEnum, Flatten};

use crate::postgres::PostgresDB;
use crate::postgres::row::{bind_postgres_value, row_to_record};
use crate::postgres::types::AnyPostgresType;

impl vantage_expressions::ExprDataSource<AnyPostgresType> for PostgresDB {
    async fn execute(
        &self,
        expr: &Expression<AnyPostgresType>,
    ) -> vantage_core::Result<AnyPostgresType> {
        // 1. Resolve deferred parameters (async — may call other databases)
        let resolved = resolve_deferred(expr).await?;

        // 2. Flatten nested expressions + convert {} to $N (PostgreSQL uses $1, $2, ...)
        let (sql, params) = prepare_typed_query(&resolved);

        // 3. Bind and execute
        let mut query = sqlx::query(&sql);
        for value in &params {
            query = bind_postgres_value(query, value);
        }

        let rows = query.fetch_all(self.pool()).await.map_err(|e| {
            vantage_core::error!("PostgreSQL query failed", details = e.to_string())
        })?;

        // 4. Convert rows to AnyPostgresType — each row becomes a CBOR Map
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
        Ok(AnyPostgresType::from_cbor(&cbor_arr)
            .expect("CBOR array should always convert to AnyPostgresType"))
    }

    fn defer(&self, expr: Expression<AnyPostgresType>) -> DeferredFn<AnyPostgresType> {
        let db = self.clone();
        DeferredFn::from_fn(move || {
            let db = db.clone();
            let expr = expr.clone();
            Box::pin(async move {
                let result = vantage_expressions::ExprDataSource::execute(&db, &expr).await?;
                Ok(match result.value() {
                    CborValue::Array(arr) => arr
                        .first()
                        .and_then(|row| match row {
                            CborValue::Map(map) => map
                                .first()
                                .map(|(_, v)| AnyPostgresType::untyped(v.clone())),
                            _ => None,
                        })
                        .unwrap_or(result),
                    _ => result,
                })
            })
        })
    }
}

/// Resolve all Deferred parameters in an expression by calling them.
async fn resolve_deferred(
    expr: &Expression<AnyPostgresType>,
) -> vantage_core::Result<Expression<AnyPostgresType>> {
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

/// Flatten an Expression<AnyPostgresType> and convert `{}` placeholders to `$N`.
/// PostgreSQL uses $1, $2, ... for positional parameters (not ?N like SQLite).
fn prepare_typed_query(expr: &Expression<AnyPostgresType>) -> (String, Vec<AnyPostgresType>) {
    let flattener = ExpressionFlattener::new();
    let flattened = flattener.flatten(expr);

    let mut sql = String::new();
    let mut params = Vec::new();
    let template_parts: Vec<&str> = flattened.template.split("{}").collect();
    let mut param_counter = 0;

    assert_eq!(
        template_parts.len(),
        flattened.parameters.len() + 1,
        "template placeholder count ({}) doesn't match parameter count ({})",
        template_parts.len() - 1,
        flattened.parameters.len()
    );

    sql.push_str(template_parts[0]);

    for (i, param) in flattened.parameters.iter().enumerate() {
        match param {
            ExpressiveEnum::Scalar(value) => {
                param_counter += 1;
                sql.push_str(&format!("${}", param_counter));
                params.push(value.clone());
            }
            ExpressiveEnum::Nested(_) => {
                panic!("nested expression should have been flattened");
            }
            ExpressiveEnum::Deferred(_) => {
                panic!("deferred expression should have been resolved before prepare");
            }
        }

        if i + 1 < template_parts.len() {
            sql.push_str(template_parts[i + 1]);
        }
    }

    (sql, params)
}
