//! `MockSelectableDataSource`: a `SelectableDataSource` that answers every select
//! with one value.

use crate::mocks::select::MockSelect;
use crate::traits::datasource::{DataSource, SelectableDataSource};
use serde_json::Value;
use vantage_core::Result;

/// A `SelectableDataSource` that returns the same rows for every select: the
/// items of an array value, or a single row for any other value.
#[derive(Debug, Clone)]
pub struct MockSelectableDataSource {
    value: Value,
}

impl MockSelectableDataSource {
    /// A source that returns `value` (as rows) for every select.
    pub fn new(value: Value) -> Self {
        Self { value }
    }
}

impl DataSource for MockSelectableDataSource {}

impl SelectableDataSource<serde_json::Value> for MockSelectableDataSource {
    type Select = MockSelect;

    fn select(&self) -> Self::Select {
        MockSelect::new()
    }

    async fn execute_select(&self, _select: &Self::Select) -> Result<Vec<serde_json::Value>> {
        if let Value::Array(arr) = &self.value {
            Ok(arr.clone())
        } else {
            Ok(vec![self.value.clone()])
        }
    }
}
