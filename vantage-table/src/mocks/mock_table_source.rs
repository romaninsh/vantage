use std::sync::{Arc, Mutex};

use serde_json::Value;
use vantage_dataset::{
    WritableValueSet,
    im::{ImDataSource, ImTable},
};
use vantage_expressions::{
    mocks::datasource::MockSelectableDataSource, traits::datasource::DataSource,
};
use vantage_types::Record;

mod conditions;
mod expr_sources;
mod table_source;
mod writes;

/// An in-memory `TableSource` for tests.
///
/// All row storage lives in a single [`ImDataSource`] — there is no second
/// count store to drift out of sync, so `get_table_count` always agrees with
/// what `list`/`get` return, including after writes.
///
/// Meets the write contract for literal `column = value` conditions; any
/// other condition on a by-id write is refused with `Unsupported`. Reads
/// also evaluate the search condition `search_table_condition` builds and
/// refuse any other condition with `Unsupported`.
#[derive(Clone)]
pub struct MockTableSource {
    im_data_source: ImDataSource,
    select_source: Option<MockSelectableDataSource>,
    query_source: Option<Arc<Mutex<vantage_expressions::mocks::mock_builder::MockBuilder>>>,
    confines: bool,
}

impl MockTableSource {
    pub fn new() -> Self {
        Self {
            im_data_source: ImDataSource::new(),
            select_source: None,
            query_source: None,
            confines: true,
        }
    }

    /// Report [`TableSource::can_confine_writes`](crate::traits::table_source::TableSource::can_confine_writes)
    /// as `false`, standing in for a backend that cannot keep by-id writes
    /// inside a conditioned set.
    pub fn without_confined_writes(mut self) -> Self {
        self.confines = false;
        self
    }

    pub async fn with_data(self, table_name: &str, data: Vec<Value>) -> Self {
        // Seed rows straight into the single store. Every row must carry a
        // scalar `id`: setup that doesn't fails loudly rather than dropping
        // the row, which would make count and list disagree.
        let im_table = ImTable::<vantage_types::EmptyEntity>::new(&self.im_data_source, table_name);
        for value in data.iter() {
            let id_str = match value.get("id") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                Some(other) => panic!(
                    "MockTableSource::with_data: row in '{table_name}' has a non-scalar `id` ({other:?}); ids must be a string or number"
                ),
                None => panic!(
                    "MockTableSource::with_data: row in '{table_name}' has no `id` field; every seed row must carry a scalar id"
                ),
            };
            let record = Record::from(value.clone());
            im_table
                .replace_value(&id_str, &record)
                .await
                .expect("Unable to replace value in im_table");
        }

        self
    }

    pub fn with_select_source(mut self, select_source: MockSelectableDataSource) -> Self {
        self.select_source = Some(select_source);
        self
    }

    pub fn with_query_source(
        mut self,
        query_source: vantage_expressions::mocks::mock_builder::MockBuilder,
    ) -> Self {
        self.query_source = Some(Arc::new(Mutex::new(query_source)));
        self
    }
}

impl Default for MockTableSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for MockTableSource {}

#[cfg(test)]
mod tests;
