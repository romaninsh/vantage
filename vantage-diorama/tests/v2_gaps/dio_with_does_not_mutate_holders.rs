//! Deriving a narrowed Dio must leave the original alone.
//!
//! `derive` builds a new Dio over a narrowed master, and the original keeps
//! its own query. Both Dios warm through the Lens's blocking eager start,
//! which lists each one's own master, so the derived Dio's cache is seeded
//! independently.

use vantage_core::Result;
use vantage_vista::{Condition, FilterOp};

use super::fixtures::scenery_ids;
use crate::support::{eager_dio, teams_master, text};

#[tokio::test]
async fn dio_with_does_not_mutate_holders() -> Result<()> {
    // Members a (red), b (blue), c (red).
    let a = eager_dio(teams_master()).await;
    let b = a
        .derive()
        .with_condition(Condition::new("team", FilterOp::Eq, text("red")))
        .build()
        .await?;

    let from_a = a.table_scenery().open().await?;
    let from_b = b.table_scenery().open().await?;

    assert_eq!(
        (scenery_ids(&from_a), scenery_ids(&from_b)),
        (
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
            vec!["a".to_string(), "c".to_string()],
        ),
        "the original Dio must stay un-narrowed; only the derived one is red-only"
    );
    #[cfg(feature = "diagnostics")]
    assert_ne!(
        a.diagnostics().await?.cache_table,
        b.diagnostics().await?.cache_table
    );
    Ok(())
}
