//! A Lens with a detail pass but no list pass must not leave its sceneries
//! loading forever.
//!
//! `on_load_detail` alone opts a Dio into two-pass loading, but the list
//! page bails when neither augmentation nor `on_list_page` exists. Nothing
//! then settles the view, and it stays `LoadState::Loading`.
//!
//! In 2.0 both hooks are removed and two-pass is declared only through
//! `augment`, so this half-configured Lens can't be built: the expected
//! outcome is a compile error. Today's API can still express the Lens, so
//! the test accepts the nearest observable equivalents: `build()` refusing
//! it, or a scenery that settles (with rows or an error) within a timeout.

use std::sync::Arc;
use std::time::Duration;

use vantage_core::Result;
use vantage_diorama::{Lens, LoadState};

use crate::support::{record, teams_master};

#[tokio::test]
#[ignore = "v2 phase 3: hand-rolled two-pass hooks are removed; on_load_detail alone never settles"]
async fn two_pass_detail_without_list_page() -> Result<()> {
    let built = Lens::new()
        .cache_in_memory()
        .viewport_debounce(Duration::from_millis(1))
        .on_load_detail(|_dio, id| async move { Ok(record(&[("id", &id)])) })
        .build();
    let Ok(lens) = built else {
        // Refused at build: the 2.0 rule, enforced at runtime.
        return Ok(());
    };
    let dio = Arc::new(lens).make_dio(teams_master()).await?;

    let scenery = dio.table_scenery().page_size(10).open().await?;
    scenery.set_viewport(0..10);
    let mut generations = scenery.subscribe();
    let settled = tokio::time::timeout(Duration::from_secs(5), async {
        while scenery.load_state() == LoadState::Loading {
            if generations.changed().await.is_err() {
                return;
            }
        }
    })
    .await;

    // A closed channel ends the wait early, so check the state as well.
    assert!(
        settled.is_ok() && scenery.load_state() != LoadState::Loading,
        "the scenery is still {:?} (waited up to 5s); a Lens with on_load_detail and no \
         on_list_page must be refused or settle",
        scenery.load_state()
    );
    Ok(())
}
