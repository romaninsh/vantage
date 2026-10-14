//! A Dio derived with an order its master can't apply keeps the order in its
//! residual, and every read shows it: `fetch` hands it back in `refused`,
//! and the facade and a table scenery sort by it.

use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::{Capability, SortDirection, VistaQuery};

use super::fixtures::{items, open, wait_for_ids};
use crate::support::eager_dio;

#[tokio::test]
async fn a_residual_order_is_one_order_across_every_read() -> Result<()> {
    let dio = eager_dio(items(&[Capability::Search, Capability::FilterOps])).await;
    let by_n = dio
        .derive()
        .with_order("n", SortDirection::Descending)
        .build()
        .await?;
    assert!(by_n.residual().order.is_some(), "the master refused it");

    let fetched = by_n.fetch(0..3, &VistaQuery::new()).await?;
    assert_eq!(
        fetched.refused.order,
        Some(("n".to_string(), SortDirection::Descending)),
        "the page isn't in the Dio's order, and says so"
    );

    let listed: Vec<String> = by_n.cached().list_values().await?.into_keys().collect();
    assert_eq!(listed, ["c", "b", "a"]);

    let view = open(&by_n, |builder| builder).await;
    wait_for_ids(&view, &["c", "b", "a"]).await;
    Ok(())
}
