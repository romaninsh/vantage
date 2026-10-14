//! Following a parent live across the narrowing filter, and `live()` over
//! a master that can't be followed.

use vantage_core::Result;
use vantage_vista::{Capability, Vista};

use super::{Parent, eager_dio, holds, in_transit, master, shipment, wait_until};

#[tokio::test]
async fn a_narrowed_dio_follows_its_parent_across_the_filter() -> Result<()> {
    let shell = master();
    shell.set_record("s1", shipment("In transit"));
    shell.set_record("s2", shipment("Delivered"));
    let parent = Parent::over(shell).await?;
    let derived = eager_dio(in_transit(&parent.dio)?, true).await?;

    assert!(derived.master().capabilities().has(Capability::Subscribe));
    assert!(holds(&derived, "s1").await);
    assert!(!holds(&derived, "s2").await);

    // Patched into the filter: appears downstream with the new value.
    let bus = derived.subscribe_events();
    parent.upsert("s2", shipment("In transit")).await?;
    wait_until(&derived, bus, async |d| holds(d, "s2").await).await;

    // Patched out of the filter: removed downstream.
    let bus = derived.subscribe_events();
    parent.upsert("s1", shipment("Delivered")).await?;
    wait_until(&derived, bus, async |d| !holds(d, "s1").await).await;

    // Removed from the parent: removed downstream.
    let bus = derived.subscribe_events();
    parent.delete("s2").await?;
    wait_until(&derived, bus, async |d| !holds(d, "s2").await).await;
    Ok(())
}

/// `live()` asks to follow the master "when it can": over a master without
/// `Subscribe` the build succeeds and the Dio simply doesn't follow.
#[tokio::test]
async fn live_over_a_master_without_subscribe_builds() -> Result<()> {
    let shell = master();
    shell.set_record("s1", shipment("In transit"));
    let master = Vista::new("shipment", shell);
    assert!(!master.capabilities().has(Capability::Subscribe));
    let dio = eager_dio(master, true).await?;
    assert!(holds(&dio, "s1").await);
    Ok(())
}
