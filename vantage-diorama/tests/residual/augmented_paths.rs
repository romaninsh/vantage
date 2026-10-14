//! A dotted path under an augmented column (`detail.size` under `detail`)
//! is an augmented clause: diorama keeps it and evaluates it over hydrated
//! rows. It is never offered to the master, which has never heard of
//! `detail` and, like Mongo or SurrealDB, might take any path and filter on
//! a field it doesn't hold.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::{Cache, Dio, LoadState};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Column, Condition, FilterOp, SortDirection, TableShell, Vista, VistaMetadata,
    forward_table_shell,
};

use super::fixtures::{open, wait_for_ids};
use crate::support::{augmented_dio, cond, fixed_augmentation, quick_lens};

/// `(id, detail.size)`.
const SIZES: [(&str, i64); 4] = [("o1", 3), ("o2", 5), ("o3", 1), ("o4", 5)];

/// A master that takes a condition or an order on any column, path or not,
/// and logs each one it was offered. It narrows nothing.
#[derive(Clone)]
struct TakesAnyPath {
    inner: Box<dyn TableShell>,
    offered: Arc<Mutex<Vec<String>>>,
}

forward_table_shell!(TakesAnyPath, inner, {
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.offered.lock().unwrap().push(condition.column.clone());
        Ok(())
    }
    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        if let Some((column, _)) = order {
            self.offered.lock().unwrap().push(column.to_string());
        }
        Ok(())
    }
});

fn row(pairs: Vec<(&str, CborValue)>) -> Record<CborValue> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

/// A Dio over [`TakesAnyPath`] listing `o1`..`o4`, augmented with a map
/// column `detail` holding `size`, and the master's offered log.
async fn detail_dio() -> (Dio, Arc<Mutex<Vec<String>>>) {
    let master_meta = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    let master = SIZES.iter().fold(
        MockShell::new().with_metadata(master_meta),
        |shell, (id, _)| shell.with_record(*id, row(vec![("id", text(id))])),
    );
    let offered = Arc::new(Mutex::new(Vec::new()));
    let log = offered.clone();
    let master = Vista::new("bucket", master).map_shell(move |inner| TakesAnyPath {
        inner,
        offered: log,
    });

    let detail_meta = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("detail", "map"))
        .with_id_column("id");
    let detail = SIZES.iter().fold(
        MockShell::new().with_metadata(detail_meta),
        |shell, (id, size)| {
            let detail = CborValue::Map(vec![(text("size"), CborValue::Integer((*size).into()))]);
            shell.with_record(*id, row(vec![("id", text(id)), ("detail", detail)]))
        },
    );
    let augmentation = fixed_augmentation(Vista::new("details", detail.get_only()), &["detail"]);
    let dio = augmented_dio(&quick_lens(Cache::memory()), master, vec![augmentation]).await;
    (dio, offered)
}

#[tokio::test]
async fn a_dotted_condition_under_an_augmented_column_stays_in_the_residual() -> Result<()> {
    let (dio, offered) = detail_dio().await;
    let fives = dio
        .derive()
        .with_query(&cond("detail.size", FilterOp::Eq, 5))
        .build()
        .await?;
    assert_eq!(fives.residual().conditions.len(), 1, "kept by diorama");
    assert!(fives.master().query().conditions.is_empty());

    let view = open(&fives, |b| b.page_size(10)).await;
    wait_for_ids(&view, &["o2", "o4"]).await;
    assert!(
        offered.lock().unwrap().is_empty(),
        "the master was offered {:?}",
        offered.lock().unwrap()
    );
    Ok(())
}

#[tokio::test]
async fn a_view_ordered_by_a_dotted_augmented_path_sweeps_rather_than_fails() -> Result<()> {
    let (dio, offered) = detail_dio().await;
    let view = open(&dio, |b| {
        b.page_size(10)
            .order("detail.size", SortDirection::Ascending)
    })
    .await;
    wait_for_ids(&view, &["o3", "o1", "o2", "o4"]).await;
    assert!(!matches!(view.load_state(), LoadState::Failed(_)));
    assert!(
        offered.lock().unwrap().is_empty(),
        "the master was offered {:?}",
        offered.lock().unwrap()
    );
    Ok(())
}
