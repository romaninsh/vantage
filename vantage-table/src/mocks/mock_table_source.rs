use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use vantage_core::Result;
use vantage_dataset::{
    WritableValueSet,
    im::{ImDataSource, ImTable},
};
use vantage_expressions::{
    mocks::datasource::MockSelectableDataSource, traits::datasource::DataSource,
};
use vantage_types::Record;
use vantage_vista::Transaction;

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
    coercion: Option<ColumnCoercion>,
    faults: Arc<WriteFaults>,
    transaction: Option<Transaction>,
}

type ColumnCoercion = Arc<dyn Fn(&str, Value) -> Result<Value> + Send + Sync>;

/// Shared by every clone, so a table's copy of the source sees them too.
#[derive(Default)]
struct WriteFaults {
    attempts: AtomicUsize,
    unavailable: AtomicUsize,
    lost_replies: AtomicUsize,
}

impl MockTableSource {
    pub fn new() -> Self {
        Self {
            im_data_source: ImDataSource::new(),
            select_source: None,
            query_source: None,
            confines: true,
            coercion: None,
            faults: Arc::default(),
            transaction: None,
        }
    }

    /// Fail the next `n` writes `Unavailable` before they touch the store,
    /// standing in for a backend that couldn't be reached.
    pub fn with_unavailable_writes(self, n: usize) -> Self {
        self.faults.unavailable.store(n, Ordering::SeqCst);
        self
    }

    /// Store the next `n` writes, then fail them `Unavailable`, standing in
    /// for a reply lost after the backend applied the write.
    pub fn with_lost_write_replies(self, n: usize) -> Self {
        self.faults.lost_replies.store(n, Ordering::SeqCst);
        self
    }

    /// How many writes (insert, replace, patch, delete, delete-all) reached
    /// this source, failed ones included, across all its clones.
    pub fn write_attempts(&self) -> usize {
        self.faults.attempts.load(Ordering::SeqCst)
    }

    /// Report [`TableSource::transaction`](crate::traits::table_source::TableSource::transaction)
    /// as `tx`, standing in for a datasource in transaction mode. Writes
    /// still go straight to the store.
    pub fn with_transaction(mut self, tx: Transaction) -> Self {
        self.transaction = Some(tx);
        self
    }

    /// Runs `write` unless a fault is due, counting the attempt either way.
    async fn faulty<R>(&self, write: impl Future<Output = Result<R>>) -> Result<R> {
        self.faults.attempts.fetch_add(1, Ordering::SeqCst);
        if take_one(&self.faults.unavailable) {
            return Err(vantage_core::error!("mock backend unreachable").mark_unavailable());
        }
        let out = write.await?;
        if take_one(&self.faults.lost_replies) {
            return Err(vantage_core::error!("mock backend reply lost").mark_unavailable());
        }
        Ok(out)
    }

    /// Answer [`TableSource::coerce_column_value`](crate::traits::table_source::TableSource::coerce_column_value)
    /// with `coerce(column, value)`, standing in for a backend that types
    /// written values by column.
    pub fn with_column_coercion(
        mut self,
        coerce: impl Fn(&str, Value) -> Result<Value> + Send + Sync + 'static,
    ) -> Self {
        self.coercion = Some(Arc::new(coerce));
        self
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

fn take_one(counter: &AtomicUsize) -> bool {
    counter
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok()
}

impl Default for MockTableSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for MockTableSource {}
