#![allow(async_fn_in_trait)]

use crate::sql::Query;
use anyhow::Result;
use serde_json::{Map, Value};

pub trait DataSource: Clone + Send + PartialEq + Sync + std::fmt::Debug + 'static {
    // Provided with an arbitrary query, fetch the results and return (Value = arbytrary )
    async fn query_fetch(&self, query: &Query) -> Result<Vec<Map<String, Value>>>;

    // Execute a query without returning any results (e.g. DELETE, UPDATE, ALTER, etc.)
    async fn query_exec(&self, query: &Query) -> Result<Option<Value>>;

    // Insert ordered list of rows into a table as described by query columns
    async fn query_insert(&self, query: &Query, rows: Vec<Vec<Value>>) -> Result<()>;

    async fn query_one(&self, query: &Query) -> Result<Value>;
    async fn query_row(&self, query: &Query) -> Result<Map<String, Value>>;
    async fn query_col(&self, query: &Query) -> Result<Vec<Value>>;
}
