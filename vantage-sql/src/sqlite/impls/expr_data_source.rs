use serde_json::Value as JsonValue;
use vantage_expressions::traits::expressive::DeferredFn;
use vantage_expressions::{Expression, ExpressionFlattener, Flatten};

use crate::sqlite::types::AnySqliteType;
use crate::sqlite::SqliteDB;
use crate::sqlite::row::{bind_sqlite_value, row_to_record};

impl vantage_expressions::ExprDataSource<AnySqliteType> for SqliteDB {
    async fn execute(
        &self,
        expr: &Expression<AnySqliteType>,
    ) -> vantage_core::Result<AnySqliteType> {
        let (sql, params) = prepare_typed_query(expr);

        let mut query = sqlx::query(&sql);
        for value in &params {
            query = bind_sqlite_value(query, value);
        }

        let rows = query
            .fetch_all(self.pool())
            .await
            .map_err(|e| vantage_core::error!("SQLite query failed", details = e.to_string()))?;

        // Each row becomes a Record<AnySqliteType> (marker-less values).
        // We wrap the whole result as a JSON array → AnySqliteType with type_variant: None.
        let arr: Vec<JsonValue> = rows
            .iter()
            .map(|row| {
                let record = row_to_record(row);
                // Convert Record<AnySqliteType> → JSON object by extracting inner values
                let json_map: serde_json::Map<String, JsonValue> = record
                    .into_iter()
                    .map(|(k, v)| (k, v.into_value()))
                    .collect();
                JsonValue::Object(json_map)
            })
            .collect();

        let json_arr = JsonValue::Array(arr);
        // from_json on an array → type_variant: None (not in our variant list)
        // This is intentional — the result is an opaque container, not a typed value.
        Ok(AnySqliteType::from_json(&json_arr)
            .expect("JSON array should always convert to AnySqliteType"))
    }

    fn defer(&self, expr: Expression<AnySqliteType>) -> DeferredFn<AnySqliteType> {
        let db = self.clone();
        DeferredFn::from_fn(move || {
            let db = db.clone();
            let expr = expr.clone();
            Box::pin(async move {
                vantage_expressions::ExprDataSource::execute(&db, &expr).await
            })
        })
    }
}

/// Flatten an Expression<AnySqliteType> and convert `{}` placeholders to `?N`.
fn prepare_typed_query(
    expr: &Expression<AnySqliteType>,
) -> (String, Vec<AnySqliteType>) {
    let flattener = ExpressionFlattener::new();
    let flattened = flattener.flatten(expr);

    let mut sql = String::new();
    let mut params = Vec::new();
    let template_parts: Vec<&str> = flattened.template.split("{}").collect();
    let mut param_counter = 0;

    sql.push_str(template_parts[0]);

    for (i, param) in flattened.parameters.iter().enumerate() {
        match param {
            vantage_expressions::ExpressiveEnum::Scalar(value) => {
                param_counter += 1;
                sql.push_str(&format!("?{}", param_counter));
                params.push(value.clone());
            }
            vantage_expressions::ExpressiveEnum::Nested(_) => {
                panic!("nested expression should have been flattened");
            }
            vantage_expressions::ExpressiveEnum::Deferred(_) => {
                panic!("deferred expression should have been resolved");
            }
        }

        if i + 1 < template_parts.len() {
            sql.push_str(template_parts[i + 1]);
        }
    }

    (sql, params)
}
