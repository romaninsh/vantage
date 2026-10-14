use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};

use crate::{DataSet, TableStoreError, VantageTableAdapter};

#[tokio::test]
async fn a_failed_load_is_a_fetch_error_not_an_empty_table() {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    let shell = MockShell::new().with_metadata(metadata);
    shell.set_fail_reads(true);
    let adapter = VantageTableAdapter::new(Vista::new("items", shell)).await;

    assert!(matches!(
        adapter.row_count().await,
        Err(TableStoreError::FetchError(_))
    ));
    assert!(matches!(
        adapter.fetch_rows(0, 10).await,
        Err(TableStoreError::FetchError(_))
    ));
}
