//! `sql::concat` and `sql::concat_ws` from Rhai, rendered and read back.
//!
//! The invoking module defines `async fn connect()`, `fn quote(name)` and
//! `TEXT`, its text column type. The macro takes the dialect's engine types
//! and the template `concat([ident("a"), ":", ident("b")])` flattens to.
//! Each test reads one row of its own `concat_<test>` table with integer
//! `a`, `b` and text `city`, `zip` columns.

macro_rules! rhai_concat_tests {
    ($db:ty, $value:ty, $select:ty, $join:ty, $cond:ty, $rendered:expr) => {
        #[cfg(feature = "rhai")]
        mod rhai_concat {
            use super::*;
            use ciborium::Value as Cbor;
            use vantage_expressions::{
                ExprDataSource, ExpressionFlattener, ExpressiveEnum, Flatten,
            };
            use vantage_types::Record;

            vantage_sql::register_engine!(
                value: $value,
                select: $select,
                join: $join,
                cond: $cond,
            );

            fn eval(code: &str) -> Result<Expression<$value>, String> {
                __host()
                    .compile(&vantage_rhai::Block::from(code))
                    .and_then(|script| {
                        script.eval_as::<vantage_sql::rhai_engine::RhaiExpr<$value>>(
                            &vantage_rhai::Env::new(),
                        )
                    })
                    .map(|e| e.0)
                    .map_err(|e| e.to_string())
            }

            /// What `code` reads over the one row `(a, b, city, zip) = row`.
            async fn read(test: &str, row: &str, code: &str) -> Cbor {
                let db: $db = connect().await;
                let table = quote(&format!("concat_{test}"));
                for sql in [
                    format!("DROP TABLE IF EXISTS {table}"),
                    format!(
                        "CREATE TABLE {table} (a INTEGER, b INTEGER, city {TEXT}, zip {TEXT})"
                    ),
                    format!("INSERT INTO {table} VALUES {row}"),
                ] {
                    sqlx::query(sqlx::AssertSqlSafe(sql))
                        .execute(db.pool())
                        .await
                        .unwrap();
                }
                let select = Expression::new(
                    format!("SELECT {{}} AS v FROM {table}"),
                    vec![ExpressiveEnum::Nested(eval(code).unwrap())],
                );
                let rows = Vec::<Record<$value>>::try_from(db.execute(&select).await.unwrap())
                    .unwrap();
                rows[0]["v"].value().clone()
            }

            fn text(s: &str) -> Cbor {
                Cbor::Text(s.to_string())
            }

            #[test]
            fn rhai_concat_renders_per_dialect() {
                let e = eval(r#"sql::concat([sql::ident("a"), ":", sql::ident("b")])"#).unwrap();
                let flat = ExpressionFlattener::new().flatten(&e);
                assert_eq!(flat.template, $rendered);
                let [ExpressiveEnum::Scalar(bound)] = flat.parameters.as_slice() else {
                    panic!("the string is one bound parameter: {:?}", flat.parameters);
                };
                assert_eq!(bound.try_get::<String>().as_deref(), Some(":"));
            }

            #[tokio::test]
            async fn rhai_concat_reads_joined_value() {
                let v = read(
                    "reads_joined_value",
                    "(7, 5, NULL, NULL)",
                    r#"sql::concat([sql::ident("a"), ":", sql::ident("b")])"#,
                )
                .await;
                assert_eq!(v, text("7:5"));
            }

            #[tokio::test]
            async fn rhai_concat_null_part_is_null() {
                let v = read(
                    "null_part_is_null",
                    "(7, NULL, NULL, NULL)",
                    r#"sql::concat([sql::ident("a"), ":", sql::ident("b")])"#,
                )
                .await;
                assert_eq!(v, Cbor::Null);
            }

            #[tokio::test]
            async fn rhai_concat_ws_skips_nulls() {
                let v = read(
                    "ws_skips_nulls",
                    "(NULL, NULL, 'Riga', NULL)",
                    r#"sql::concat_ws(", ", [sql::ident("city"), sql::ident("zip")])"#,
                )
                .await;
                assert_eq!(v, text("Riga"));
            }

            #[tokio::test]
            async fn rhai_concat_string_is_never_sql() {
                let v = read(
                    "string_is_never_sql",
                    "(NULL, NULL, NULL, NULL)",
                    r#"sql::concat(["'; DROP TABLE x; --"])"#,
                )
                .await;
                assert_eq!(v, text("'; DROP TABLE x; --"));
            }

            #[test]
            fn rhai_concat_rejects_unknown_part() {
                let err = eval(r#"sql::concat([sql::ident("a"), #{ b: 1 }])"#).unwrap_err();
                assert!(err.contains("map") && err.contains("position 2"), "{err}");
            }
        }
    };
}
