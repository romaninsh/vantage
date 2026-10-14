//! Recovering the follow: a lagged child re-reads the set, and a re-read
//! that fails is published as a failed refresh.

use std::time::Duration;

use futures::StreamExt;
use tokio::sync::broadcast;
use vantage_core::Result;
use vantage_diorama::{Cache, Dio, DioEvent, Lens};
use vantage_vista::{
    Capability, TableShell, Vista, VistaCapabilities, VistaChange, VistaChangeStream,
    forward_table_shell,
};

use super::{Parent, eager_dio, in_transit, master, shipment, wait_until};

/// A narrowed child whose follow falls more than a bus's worth of events
/// behind its parent lags, re-reads the set, and catches up.
///
/// The parent's changes are applied without yielding, so on this
/// single-threaded runtime the child's follow can't read any of them until
/// all are published: its subscription to the parent's bus overflows.
#[tokio::test]
async fn a_lagged_narrowed_child_catches_up() -> Result<()> {
    const ROWS: usize = 200;
    let parent = Parent::over(master()).await?;
    let child = eager_dio(in_transit(&parent.dio)?, true).await?;
    let bus = child.subscribe_events();
    // The recovery goes through a re-read, which announces itself first.
    let mut refreshes = child.subscribe_events();
    let reread = tokio::spawn(async move {
        loop {
            match refreshes.recv().await {
                Ok(DioEvent::Refreshing) => return true,
                Err(broadcast::error::RecvError::Closed) => return false,
                _ => {}
            }
        }
    });

    for i in 0..ROWS {
        let status = if i % 2 == 0 {
            "In transit"
        } else {
            "Delivered"
        };
        parent.upsert(&format!("s{i:03}"), shipment(status)).await?;
    }

    let caught_up = async |d: &Dio| d.cached().count().await.ok() == Some((ROWS / 2) as i64);
    wait_until(&child, bus, caught_up).await;
    let reread = tokio::time::timeout(Duration::from_secs(5), reread).await;
    assert!(
        reread.expect("no re-read was announced").expect("listener"),
        "the child recovered through a re-read"
    );
    Ok(())
}

/// A watchable master whose stream yields one `Invalidated`, then stays open.
#[derive(Clone)]
struct InvalidatingShell {
    inner: Box<dyn TableShell>,
    capabilities: VistaCapabilities,
}

forward_table_shell!(InvalidatingShell, inner, {
    async fn watch(&self) -> Result<VistaChangeStream> {
        let invalidated = futures::stream::once(async { Ok(VistaChange::Invalidated) });
        Ok(Box::pin(invalidated.chain(futures::stream::pending())))
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
});

/// The master invalidates the set and can't be read: the follow's re-read
/// fails, and the bus says so as a failed refresh, not as a failed range load.
#[tokio::test]
async fn a_failed_recovery_reread_publishes_refresh_failed() -> Result<()> {
    let shell = master();
    shell.set_fail_reads(true);
    let capabilities = shell.capabilities().with(Capability::Subscribe);
    let vista = Vista::new("shipment", shell).map_shell(|inner| InvalidatingShell {
        inner,
        capabilities,
    });
    let lens = Lens::builder().cache(Cache::memory()).build()?;
    let dio = lens.dio(vista).live().build().await?;
    let mut bus = dio.subscribe_events();

    let seen = tokio::time::timeout(Duration::from_secs(3), async {
        let mut seen = Vec::new();
        loop {
            match bus.recv().await {
                Ok(DioEvent::RefreshFailed { error }) => return (error, seen),
                Ok(event) => seen.push(event),
                Err(e) => panic!("bus: {e}"),
            }
        }
    })
    .await;
    let (error, before) = seen.expect("the failed re-read is published");
    assert!(error.contains("injected"), "{error}");
    let stand_in =
        |e: &DioEvent| matches!(e, DioEvent::LoadFailed { range, .. } if range.is_empty());
    assert!(!before.iter().any(stand_in), "{before:?}");
    Ok(())
}
