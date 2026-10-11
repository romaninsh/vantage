//! Two opens of the same query must not let one reshape the other.
//!
//! Today the dedup registry hands the second open the same live scenery as
//! the first, and the setters take `&self`, so `set_search` on one holder
//! changes the rows of every holder. `.exclusive()` is the opt-out. In 2.0
//! every `open()` returns a scenery its caller owns; only the fetched rows
//! are shared underneath, and only between identical queries.

use std::time::Duration;

use vantage_core::Result;

use super::fixtures::{eventually, scenery_ids};
use crate::support::{eager_dio, teams_master};

#[tokio::test]
#[ignore = "v2 phase 4: shared sceneries let one holder reshape another"]
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
