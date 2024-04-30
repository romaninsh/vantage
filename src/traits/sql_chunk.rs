use crate::expression::Expression;
use serde_json::Value;
use std::fmt::Debug;

// use crate::operations::Operations;

/// A `SqlChunk` trait for generating SQL queries and their associated parameters
///
/// This trait is designed to allow various types of SQL statements or sub-queries
/// to be dynamically generated, including the capability to handle parameters
/// that need to be passed to the query executor.
///
/// # Examples
///
/// A simplest implementation of `SqlChunk` is an `Expression` and it should allow
/// you to do this:
///
/// ```rust
///   let a = 3; let b = 2; let c = 3
///
///   let expr2 = expr!("{} + {}", &b, &c)
///   let expr1 = expr!("{} + {}", &a, &expr2);
///
///   c = 10;
///
///   let result = client.query_one(&expr1).await?;  // 3 + (2 + 10) = 15
/// ```
///
/// In this scenario, expressions remain borrowing values of parameters until it is the time
/// to execute the query. `SqlChunk` trait makes sure that the query and all the nested
/// queries are properly nested.
///
/// The `SqlChunk` can be optionally associated with a `DataSet`. When during nesting
/// expressions will cross the boundaries of the `DataSet` - that query will be executed
/// preemptively and will be replaced with the result of the query.
///
/// Next example will execute sqlite query first, insert results into postgres query and
/// query again.
///
/// ```rust
///   let psql_client = get_psql_client();
///   let sqlite_client = get_sqlite_client();
///
///   let cached_users = expr_ds!(sqlite_client, "select id from cached_users");
///   let users = expr_ds!(psql_client, "select * from orders where user_id in ({})", &cached_users);
///
///   let result = users.fetch_all().await?;
/// ```
///
/// While `Expression` is one of the simplest implementations of `SqlChunk`, there are others:
///
/// ```rust
///   let query = table.get_select_query();
///
///   let result = query = query.fetch_all().await?;
/// ```
///
/// A `Query` can be constructed for an arbitrary query, but it makes more sense to rely on a `Table`,
/// `Union` or another implementation of `ReadableDataSet` to construct a query.
///
/// # NoSQL implementation of SqlChunk
///
/// Standard types such as String, Vec<String>, or ToSql are implementing SqlChunk and can be used
/// as a part of a query, typically resulting in
pub trait SqlChunk: Debug {
    /// Generates an SQL statement.
    ///
    /// The method should return a complete SQL statement as a `String`. An `offset`
    /// parameter is provided to allow pagination or other offset-based query features.
    ///
    /// # Parameters
    /// - `offset`: The offset value used in the SQL query, typically for pagination.
    ///
    /// # Returns
    /// - Returns a `String` that contains the SQL statement.
    fn render_chunk(&self) -> Expression;
}

impl SqlChunk for String {
    fn render_chunk(&self) -> Expression {
        Expression::new("{}".to_owned(), vec![Value::String(self.clone())])
    }
}

impl SqlChunk for Value {
    fn render_chunk(&self) -> Expression {
        Expression::new("{}".to_owned(), vec![self.clone()])
    }
}

impl SqlChunk for i64 {
    fn render_chunk(&self) -> Expression {
        Expression::new("{}".to_owned(), vec![Value::Number((*self).into())])
    }
}

impl SqlChunk for u64 {
    fn render_chunk(&self) -> Expression {
        Expression::new("{}".to_owned(), vec![Value::Number((*self).into())])
    }
}

impl SqlChunk for i32 {
    fn render_chunk(&self) -> Expression {
        Expression::new("{}".to_owned(), vec![Value::Number((*self).into())])
    }
}

impl SqlChunk for u32 {
    fn render_chunk(&self) -> Expression {
        Expression::new("{}".to_owned(), vec![Value::Number((*self).into())])
    }
}

impl SqlChunk for &str {
    fn render_chunk(&self) -> Expression {
        Expression::new("{}".to_owned(), vec![Value::String(self.to_string())])
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::expr;
    use crate::expression::Expression;

    use super::*;

    #[test]
    fn test_string_sql_chunk() {
        let s = "Hello, World!".to_owned();
        let (sql, params) = s.render_chunk().split();

        assert_eq!(sql, "{}");
        assert_eq!(params.len(), 1);
        assert_eq!(params, vec![json!("Hello, World!")])
    }

    #[test]
    fn test_pre_render_join() {
        let pre_render1 = expr!("{} + {}", 1, 2).render_chunk();
        let pre_render2 = expr!("{} + {}", 3, 4).render_chunk();

        let pre_vec = vec![pre_render1, pre_render2];
        let join = Expression::from_vec(pre_vec, " + ");

        assert_eq!(join.sql(), "{} + {} + {} + {}");
        assert_eq!(join.sql_final(), "$1 + $2 + $3 + $4");
        assert_eq!(join.params().len(), 4);
        assert_eq!(*join.params(), vec![json!(1), json!(2), json!(3), json!(4)]);
    }
}