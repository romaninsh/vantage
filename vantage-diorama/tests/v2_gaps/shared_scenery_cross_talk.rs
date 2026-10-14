//! Two opens of the same query must not let one reshape the other: every
//! `open()` returns a view its caller owns, and only the fetched rows are
//! shared underneath, between identical queries.

use std::time::Duration;

use vantage_core::Result;

use super::fixtures::scenery_ids;
use crate::support::{eager_dio, eventually, teams_master};

#[tokio::test]
async fn shared_scenery_cross_talk() -> Result<()> {
    // Members a (red), b (blue), c (red).
    let dio = eager_dio(teams_master()).await;
    let first = dio.table_scenery().open().await?;
    let second = dio.table_scenery().open().await?;

    first.set_search(Some("blue".into()));
    assert!(
        eventually(Duration::from_secs(10), || scenery_ids(&first) == ["b"]).await,
        "the searched view never narrowed to b; shows {:?}",
        scenery_ids(&first)
    );

    assert_eq!(
        scenery_ids(&second),
        ["a", "b", "c"],
        "a search on the first holder reshaped the second"
    );
    Ok(())
}
