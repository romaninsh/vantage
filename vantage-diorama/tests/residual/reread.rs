//! A refresh of a paged Dio re-reads the views served from its whole copy:
//! the finished copy is forgotten, so the next such view copies the set again.

use vantage_core::Result;
use vantage_vista::{Capability, Vista};

use super::fixtures::{metadata, open, shell, wait_for_ids};
use crate::support::paged_or_eager_dio;

/// A view searching a master that can't search is served from the whole
/// copy. Once a row leaves the master, a refresh drops it from that view.
#[tokio::test]
async fn a_refresh_makes_a_copy_served_view_copy_again() -> Result<()> {
    let master = shell(metadata(false), &[Capability::Order, Capability::FilterOps]);
    let dio = paged_or_eager_dio(Vista::new("items", master.clone())).await;
    let view = open(&dio, |b| b).await;
    view.set_search(Some("re".into()));
    wait_for_ids(&view, &["a", "c"]).await;

    master.remove_record("c");
    dio.refresh().await?;
    wait_for_ids(&view, &["a"]).await;
    Ok(())
}
