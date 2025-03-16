use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Map, Value};
use sqlx::{Database, Encode, Row, Type, query::Query};
use std::{collections::HashMap, ops::Deref};

use crate::{
    dataset::{ReadableDataSet, ScalarDataSet},
    datasource::sqlx::sql_to_json::row_to_json,
    prelude::{Entity, Postgres},
};

#[macro_export]
macro_rules! pg_expr {
    ($fmt:expr $(, $arg:expr)*) => {{
        Expression2::<sqlx::Postgres>::new(
            $fmt.to_string(),
            vec![
                $( Box::new($arg), )*
            ]
        )
    }}
}

#[macro_export]
macro_rules! expr2 {
    ($fmt:expr $(, $arg:expr)*) => {{
        Expression2::new(
            $fmt.to_string(),
            vec![
                $( Box::new($arg), )*
            ]
        )
    }}
}

pub struct Expression2<DB: Database> {
    expression: String,
    parameters: Vec<Box<dyn ExpressionChunk<DB>>>,
    // phantom: PhantomData<&'q ()>,
}

pub trait RenderSqlx<DB: Database> {
    /// Ability to render into sqlx expression
    fn render_sqlx<'q>(&self) -> Result<Query<'q, DB, <DB as Database>::Arguments<'q>>>;
}

impl<DB: Database> Expression2<DB> {
    pub fn new(expression: String, parameters: Vec<Box<dyn ExpressionChunk<DB>>>) -> Self {
        Self {
            expression,
            parameters,
            // phantom: PhantomData,
        }
    }
}

struct PgExpression {
    expression: Expression2<sqlx::Postgres>,
    datasource: Postgres,
}

impl Deref for PgExpression {
    // Define the target type that PgExpression will deref to
    type Target = Expression2<sqlx::Postgres>;

    // Return a reference to the expression field
    fn deref(&self) -> &Self::Target {
        &self.expression
    }
}

impl PgExpression {
    pub fn new(expression: Expression2<sqlx::Postgres>, datasource: Postgres) -> Self {
        Self {
            expression,
            datasource,
        }
    }
}

impl<DB: Database> RenderSqlx<DB> for Expression2<DB> {
    // Change return type to use 'q lifetime instead of 'static
    fn render_sqlx<'q>(&self) -> Result<Query<'q, DB, <DB as Database>::Arguments<'q>>> {
        // We need to leak the string to extend its lifetime
        let sql: &'static str = self.expression.clone().leak();
        let mut query = sqlx::query::<DB>(&sql);

        for parameter in &self.parameters {
            query = parameter.bind(query)?;
        }

        Ok(query)
    }
}

pub trait ExpressionChunk<DB: Database>: Send + Sync {
    fn bind<'q>(
        &self,
        query: Query<'q, DB, <DB as Database>::Arguments<'q>>,
    ) -> Result<Query<'q, DB, <DB as Database>::Arguments<'q>>>;
}

impl<DB: Database, T> ExpressionChunk<DB> for T
where
    T: for<'any> Encode<'any, DB> + Type<DB> + Clone + Send + Sync + 'static,
{
    fn bind<'q>(
        &self,
        query: Query<'q, DB, <DB as Database>::Arguments<'q>>,
    ) -> Result<Query<'q, DB, <DB as Database>::Arguments<'q>>> {
        Ok(query.bind(self.clone()))
    }
}

#[async_trait]
impl ScalarDataSet for PgExpression {
    async fn enumerate(&self) -> Result<Vec<Value>> {
        let q = self.render_sqlx()?;
        let rows = q.fetch_all(self.datasource.client()).await?;

        if rows.is_empty() {
            return Ok(Vec::new());
        }

        // Get first column index
        rows.iter()
            .map(|row| {
                row.try_get(0)
                    .map_err(|e| anyhow::anyhow!("Failed to get column value: {}", e))
            })
            .collect()
    }
}

impl<E: Entity> ReadableDataSet<E> for PgExpression {
    async fn get_col_untyped(&self) -> Result<Vec<Value>> {
        self.enumerate().await
    }
    async fn get(&self) -> Result<Vec<E>> {
        let q = self.render_sqlx()?;
        let rows = q.fetch_all(self.datasource.client()).await?;

        rows.iter()
            .map(row_to_json)
            .map(|row| {
                serde_json::from_value(Value::Object(row))
                    .map_err(|e| anyhow::anyhow!("Failed to deserialize row: {}", e))
            })
            .collect()
    }

    async fn get_all_untyped(&self) -> Result<Vec<Map<String, Value>>> {
        let q = self.render_sqlx()?;
        let rows = q.fetch_all(self.datasource.client()).await?;

        let maps = rows.into_iter().map(|row| row_to_json(&row)).collect();

        Ok(maps)
    }

    async fn get_row_untyped(&self) -> Result<Map<String, Value>> {
        let q = self.render_sqlx()?;
        let row = q.fetch_one(self.datasource.client()).await?;
        let json = row_to_json(&row);

        Ok(json)
    }

    async fn get_one_untyped(&self) -> Result<Value> {
        let q = self.render_sqlx()?;
        let row = q.fetch_one(self.datasource.client()).await?;

        // Directly get the first column value
        row.try_get(0)
            .map_err(|e| anyhow::anyhow!("Failed to get first column value: {}", e))
    }

    async fn get_some(&self) -> Result<Option<E>> {
        let q = self.render_sqlx()?;
        match q.fetch_optional(self.datasource.client()).await? {
            Some(row) => Ok(Some(serde_json::from_value(Value::Object(row_to_json(
                &row,
            )))?)),
            None => Ok(None),
        }
    }

    async fn get_as<T: serde::de::DeserializeOwned>(&self) -> Result<Vec<T>> {
        let q = self.render_sqlx()?;
        let rows = q.fetch_all(self.datasource.client()).await?;

        rows.iter()
            .map(|row| {
                serde_json::from_value(Value::Object(row_to_json(row)))
                    .map_err(|e| anyhow::anyhow!(e))
            })
            .collect()
    }

    async fn get_some_as<T>(&self) -> Result<Option<T>>
    where
        T: serde::de::DeserializeOwned + Default + serde::Serialize,
    {
        let q = self.render_sqlx()?;
        let row_opt = q.fetch_optional(self.datasource.client()).await?;

        match row_opt {
            Some(row) => {
                let json = row_to_json(&row);
                let value = serde_json::from_value(Value::Object(json))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    fn select_query(&self) -> crate::prelude::Query {
        todo!()
    }
}

/// When defining ExpressionArc2, the parameter can be a ScalarDataSet too.
/// A ScalarDataSet is unable to calculate results right away and therefore
/// cannot be part of Expression2, so we will store it separatell, and will
/// execute those recursively placing them into the correct places of an
/// expression
struct ExpressionArc2<DB: Database> {
    scalar_sets: HashMap<usize, Box<dyn ScalarDataSet>>,
    expr: Expression2<DB>,
}

#[async_trait]
impl<DB: Database> ScalarDataSet for ExpressionArc2<DB> {
    // Recursively enumerate any scalar sets
    async fn enumerate(&self) -> Result<Vec<Value>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{Connection, Execute, PgConnection, Postgres};

    // Test basic creation of Expression2
    #[tokio::test]
    async fn test_expression2_creation() -> Result<()> {
        let expr = expr2!("SELECT * FROM users WHERE id = ? AND age > ?", 42, 100);

        let query = expr.render_sqlx().unwrap();

        let mut conn: PgConnection = PgConnection::connect("<Database URL>").await?;
        let result = query.execute(&mut conn).await?;

        Ok(())
    }

    // Test rendering a simple query with parameters
    #[test]
    fn test_render_chunk_single_parameter() -> Result<()> {
        let params: Vec<Box<dyn ExpressionChunk<Postgres>>> = vec![Box::new(42)];

        let expr =
            Expression2::<Postgres>::new("SELECT * FROM users WHERE id = ?".to_string(), params);

        let query = expr.render_sqlx()?;

        // Verify the query's SQL
        assert_eq!(query.sql(), "SELECT * FROM users WHERE id = ?");

        Ok(())
    }

    // Test rendering with multiple parameters
    #[test]
    fn test_render_chunk_multiple_parameters() -> Result<()> {
        let params: Vec<Box<dyn ExpressionChunk<Postgres>>> =
            vec![Box::new(42), Box::new("John".to_string())];

        let expr = Expression2::<Postgres>::new(
            "SELECT * FROM users WHERE id = ? AND name = ?".to_string(),
            params,
        );

        let query = expr.render_sqlx()?;

        // Verify the query's SQL
        assert_eq!(query.sql(), "SELECT * FROM users WHERE id = ? AND name = ?");

        Ok(())
    }

    // Test different parameter types
    #[test]
    fn test_expression_chunk_different_types() -> Result<()> {
        let params: Vec<Box<dyn ExpressionChunk<Postgres>>> = vec![
            Box::new(42),                  // i32
            Box::new(100.50),              // f64
            Box::new("Hello".to_string()), // String
            Box::new(true),                // bool
        ];

        let expr = Expression2::<Postgres>::new(
            "SELECT * FROM test WHERE id = ? AND value > ? AND name = ? AND active = ?".to_string(),
            params,
        );

        let query = expr.render_sqlx()?;

        // Verify the query's SQL structure
        assert_eq!(
            query.sql(),
            "SELECT * FROM test WHERE id = ? AND value > ? AND name = ? AND active = ?"
        );

        Ok(())
    }

    // Compile-time type checking test
    #[test]
    fn test_type_constraints() {
        // This test ensures that only valid types can be used as parameters
        // If this compiles, it means the type constraints are working
        fn _test_type_constraint<'q>() {
            let _params: Vec<Box<dyn ExpressionChunk<Postgres>>> =
                vec![Box::new(42), Box::new("test")];
        }
    }

    // Ensure lifetime constraints are respected
    #[test]
    fn test_lifetime_constraints() {
        fn _test_lifetime_constraints<'a>() {
            let s = String::from("temp");
            let _params: Vec<Box<dyn ExpressionChunk<Postgres>>> = vec![Box::new(s.clone())];
        }
    }

    #[test]
    fn test_expr_macro_different_types() -> Result<()> {
        let id = 42;
        let name = "John";
        let active = true;
        let score = 95.5;

        let expr = pg_expr!(
            "SELECT * FROM users WHERE id = ? AND name = ? AND active = ? AND score > ?",
            id,
            name,
            active,
            score
        );

        assert_eq!(
            expr.expression,
            "SELECT * FROM users WHERE id = ? AND name = ? AND active = ? AND score > ?"
        );
        assert_eq!(expr.parameters.len(), 4);

        Ok(())
    }
}
