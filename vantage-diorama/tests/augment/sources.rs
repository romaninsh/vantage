//! Where an augmentation reads from: a resolved or fixed detail Vista, keyed
//! by the id or a master column, or a custom fetch closure.

use std::sync::Arc;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Augmentation, Detail, Fetch, MergeRule, RowStatus, Source};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};

use crate::support::{self, augmented_dio, meta, poll_until, record, status_of};
use crate::{aug, col_of, hydrated, open, redb_lens, resolved};

/// The list pass stubs rows `Incomplete` with cheap columns; no detail fetched
/// until a viewport asks. Then the detail pass merges the second vista's column
/// and flips the row `Fresh`, keeping the cheap columns.
#[tokio::test]
async fn id_source_augments_from_a_separate_vista() {
    let tmp = TempDir::new().unwrap();
    let dio = open(&tmp, vec![aug("runs-detail", Source::Id, &["detail"])]).await;
    let scenery = support::open_listed(&dio, 2).await;

    // List pass ran: cheap columns present, Incomplete, no augmented column yet.
    assert!(matches!(
        status_of(&scenery, 0),
        Some(RowStatus::Incomplete)
    ));
    assert_eq!(col_of(&scenery, 0, "branch").as_deref(), Some("main"));
    assert!(
        col_of(&scenery, 0, "detail").is_none(),
        "no detail before viewport"
    );

    scenery.set_viewport(0..2, 0..0);
    poll_until("rows hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
    })
    .await;

    // Augmented column merged from runs-detail; cheap column survived.
    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
    assert_eq!(col_of(&scenery, 1, "detail").as_deref(), Some("full-r1"));
    assert_eq!(col_of(&scenery, 0, "branch").as_deref(), Some("main"));
}

/// A `Column` source keyed on an explicit master field resolves the same way.
#[tokio::test]
async fn column_source_keyed_by_field() {
    let tmp = TempDir::new().unwrap();
    let source = Source::Column {
        from: "id".into(),
        to: None,
    };
    let dio = open(&tmp, vec![aug("runs-detail", source, &["detail"])]).await;
    let scenery = hydrated(&dio, 0).await;
    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
}

/// Multiple augmentations compose: each merges its own column from its own vista.
#[tokio::test]
async fn multiple_augmentations_merge_independently() {
    let tmp = TempDir::new().unwrap();
    let dio = open(
        &tmp,
        vec![
            aug("runs-detail", Source::Id, &["detail"]),
            aug("runs-extra", Source::Id, &["extra"]),
        ],
    )
    .await;
    let scenery = hydrated(&dio, 0).await;

    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
    assert_eq!(col_of(&scenery, 0, "extra").as_deref(), Some("x0"));
    assert_eq!(col_of(&scenery, 0, "branch").as_deref(), Some("main"));
}

/// A missing key field surfaces as a failed row — the detail pass error marks
/// only that row `LoadFailed`, and its cheap columns survive.
#[tokio::test]
async fn missing_key_field_marks_row_failed() {
    let tmp = TempDir::new().unwrap();
    let source = Source::Column {
        from: "nonexistent".into(),
        to: None,
    };
    let dio = open(&tmp, vec![aug("runs-detail", source, &["detail"])]).await;
    let scenery = support::open_listed(&dio, 2).await;
    scenery.set_viewport(0..2, 0..0);

    poll_until("row failed", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::LoadFailed { .. }))
    })
    .await;
    assert_eq!(col_of(&scenery, 0, "branch").as_deref(), Some("main"));
}

/// A caller-supplied [`Fetch::Custom`] closure reads the narrowed detail vista.
#[tokio::test]
async fn custom_fetch_closure_reads_narrowed_detail() {
    let tmp = TempDir::new().unwrap();
    let augmentation = Augmentation::new(
        resolved("runs-detail"),
        Source::Id,
        Fetch::Custom(Arc::new(|detail: Vista, _row| {
            Box::pin(async move { Ok(detail.list_values().await?.into_values().collect()) })
        })),
        MergeRule::lift(["detail"]),
    );
    let dio = open(&tmp, vec![augmentation]).await;
    let scenery = hydrated(&dio, 0).await;
    assert_eq!(col_of(&scenery, 0, "detail").as_deref(), Some("full-r0"));
}

/// A [`Fetch::Custom`] closure receives the listed master row beside the
/// narrowed detail vista: here it reads `branch`, a field the detail vista
/// doesn't have, the way cmd's row script reads its inputs.
#[tokio::test]
async fn custom_fetch_closure_reads_the_master_row() {
    let tmp = TempDir::new().unwrap();
    let augmentation = Augmentation::new(
        resolved("runs-detail"),
        Source::Id,
        Fetch::Custom(Arc::new(|detail: Vista, row| {
            Box::pin(async move {
                assert!(detail.column("branch").is_none(), "only the row has it");
                let branch = match row.get("branch") {
                    Some(CborValue::Text(b)) => b.clone(),
                    other => panic!("the listed row carries its branch: {other:?}"),
                };
                Ok(vec![record(&[("label", &format!("on {branch}"))])])
            })
        })),
        MergeRule::lift(["label"]),
    );
    let dio = open(&tmp, vec![augmentation]).await;
    let scenery = hydrated(&dio, 1).await;
    assert_eq!(col_of(&scenery, 0, "label").as_deref(), Some("on main"));
    assert_eq!(col_of(&scenery, 1, "label").as_deref(), Some("on dev"));
}

/// The finder/live_folder shape: the detail is a FIXED get-only Vista handle,
/// registered in no catalog, keyed by a master column (`path`). Rows hydrate
/// the merged columns through the same lazy detail pass.
#[tokio::test]
async fn fixed_detail_vista_hydrates_without_a_catalog() {
    let tmp = TempDir::new().unwrap();

    // Get-only side table keyed by path — the folder-size vista shape.
    let size_meta = VistaMetadata::new()
        .with_column(Column::new("path", "String").with_flag("id"))
        .with_column(Column::new("size", "String"))
        .with_column(Column::new("file_count", "String"))
        .with_id_column("path");
    let size_shell = MockShell::new()
        .with_record(
            "a/logs",
            record(&[("path", "a/logs"), ("size", "4096"), ("file_count", "3")]),
        )
        .with_record(
            "a/tmp",
            record(&[("path", "a/tmp"), ("size", "512"), ("file_count", "1")]),
        )
        .with_metadata(size_meta);
    let fixed = Arc::new(Vista::new("folder_size", size_shell));

    let master = MockShell::new()
        .with_record("r0", record(&[("id", "r0"), ("path", "a/logs")]))
        .with_record("r1", record(&[("id", "r1"), ("path", "a/tmp")]))
        .with_metadata(meta(&["id", "path"]));
    let dio = augmented_dio(
        &redb_lens(&tmp),
        Vista::new("listing", master),
        vec![Augmentation::new(
            Detail::Fixed(fixed),
            Source::Column {
                from: "path".into(),
                to: None,
            },
            Fetch::PerRow,
            MergeRule::lift(["size", "file_count"]),
        )],
    )
    .await;
    let scenery = support::open_listed(&dio, 2).await;
    scenery.set_viewport(0..2, 0..0);

    poll_until("rows hydrated", || {
        matches!(status_of(&scenery, 0), Some(RowStatus::Fresh))
            && matches!(status_of(&scenery, 1), Some(RowStatus::Fresh))
    })
    .await;
    assert_eq!(col_of(&scenery, 0, "size").as_deref(), Some("4096"));
    assert_eq!(col_of(&scenery, 0, "file_count").as_deref(), Some("3"));
    assert_eq!(col_of(&scenery, 1, "size").as_deref(), Some("512"));
    // The merge list is respected: the detail's own key column stays put.
    assert_eq!(col_of(&scenery, 0, "path").as_deref(), Some("a/logs"));
}
