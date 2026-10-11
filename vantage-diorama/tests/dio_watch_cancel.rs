//! A `watch()` task ends when its Dio drops, and drops the master's change
//! stream with it — even when the stream never yields, and even while the
//! task is backing off before a resubscribe. A stream left alive would keep
//! the backend's subscription (a server cursor, a pooled connection) busy
//! for the life of the process.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use futures::Stream;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_diorama::{Dio, Lens};
use vantage_types::Record;
use vantage_vista::reference::Reference;
use vantage_vista::source::TableShell;
use vantage_vista::{
    Column, Vista, VistaCapabilities, VistaChange, VistaChangeStream, VistaMetadata,
    mocks::MockShell,
};

/// What the test can observe about the streams a shell handed out.
#[derive(Default)]
struct Probe {
    opened: AtomicUsize,
    ended: AtomicBool,
    dropped: AtomicUsize,
}

/// A change stream that never yields a change. With `end` set it reports
/// the end of the subscription at once instead of staying pending forever.
struct QuietStream {
    probe: Arc<Probe>,
    end: bool,
}

impl Stream for QuietStream {
    type Item = Result<VistaChange>;
    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.end {
            self.probe.ended.store(true, Ordering::SeqCst);
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

impl Drop for QuietStream {
    fn drop(&mut self) {
        self.probe.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

/// A watchable master whose every `watch()` returns a [`QuietStream`].
struct QuietShell {
    inner: MockShell,
    capabilities: VistaCapabilities,
    probe: Arc<Probe>,
    end: bool,
}

#[async_trait]
#[allow(clippy::ptr_arg)]
impl TableShell for QuietShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        self.inner.columns()
    }
    fn references(&self) -> &IndexMap<String, Reference> {
        self.inner.references()
    }
    fn id_column(&self) -> Option<&str> {
        self.inner.id_column()
    }
    async fn list_vista_values(
        &self,
        vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.inner.list_vista_values(vista).await
    }
    async fn get_vista_value(
        &self,
        vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        self.inner.get_vista_value(vista, id).await
    }
    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        self.inner.get_vista_some_value(vista).await
    }
    async fn watch_vista(&self, _vista: &Vista) -> Result<VistaChangeStream> {
        self.probe.opened.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(QuietStream {
            probe: self.probe.clone(),
            end: self.end,
        }))
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    fn preview_query(&self, vista: &Vista) -> serde_json::Value {
        self.inner.preview_query(vista)
    }
}

async fn quiet_dio(end: bool) -> Result<(Dio, Arc<Probe>)> {
    let inner = MockShell::new().with_metadata(
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_id_column("id"),
    );
    let capabilities = VistaCapabilities {
        can_subscribe: true,
        ..inner.capabilities().clone()
    };
    let probe = Arc::new(Probe::default());
    let shell = QuietShell {
        inner,
        capabilities,
        probe: probe.clone(),
        end,
    };
    let lens = Arc::new(Lens::new().cache_in_memory().build().expect("lens"));
    let dio = lens.make_dio(Vista::new("quiet", Box::new(shell))).await?;
    Ok((dio, probe))
}

/// Poll `holds` until it is true, or give up after `within`.
async fn eventually(within: Duration, holds: impl Fn() -> bool) -> bool {
    tokio::time::timeout(within, async {
        while !holds() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok()
}

#[tokio::test]
async fn a_quiet_stream_is_dropped_with_its_dio() -> Result<()> {
    let (dio, probe) = quiet_dio(false).await?;
    dio.watch().await?;
    dio.watch().await?;
    assert_eq!(probe.opened.load(Ordering::SeqCst), 2);

    // Both tasks are parked on streams that will never yield.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(probe.dropped.load(Ordering::SeqCst), 0);

    drop(dio);
    assert!(
        eventually(Duration::from_millis(500), || {
            probe.dropped.load(Ordering::SeqCst) == 2
        })
        .await,
        "a change stream outlived its Dio"
    );
    Ok(())
}

#[tokio::test]
async fn a_dio_dropped_during_back_off_ends_without_waiting_it_out() -> Result<()> {
    let (dio, probe) = quiet_dio(true).await?;
    dio.watch().await?;

    // The stream ends at once, so the task is now in its 200ms back-off.
    assert!(
        eventually(Duration::from_millis(500), || probe
            .ended
            .load(Ordering::SeqCst))
        .await
    );
    drop(dio);

    // The regression guard: a task that slept out its back-off would only
    // notice the drop at 200ms, so this 100ms window catches it.
    assert!(
        eventually(Duration::from_millis(100), || {
            probe.dropped.load(Ordering::SeqCst) == 1
        })
        .await,
        "the task slept through its back-off before noticing the Dio dropped"
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        probe.opened.load(Ordering::SeqCst),
        1,
        "resubscribed after the Dio dropped"
    );
    Ok(())
}
