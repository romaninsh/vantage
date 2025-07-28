use std::fmt::Debug;

use serde_json::{Map, Value};

/// Defines different types of queries, linking them with expected output
///
/// 3 default QueryResult implementations exist:
///  - [Rows, List, Scalar]
pub trait QueryResult: Send + Sync + Debug + Clone + Copy {
    type Output;
}

#[derive(Debug, Clone, Copy)]
pub struct Rows;
#[derive(Debug, Clone, Copy)]
pub struct List;
#[derive(Debug, Clone, Copy)]
pub struct Single;

impl QueryResult for Rows {
    type Output = Vec<Map<String, Value>>;
}
impl QueryResult for List {
    type Output = Vec<Value>;
}
impl QueryResult for Single {
    type Output = Value;
}
