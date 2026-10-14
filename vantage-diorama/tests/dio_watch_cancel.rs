//! A live follow ends when its Dio drops, and drops the master's change
//! stream with it — even when the stream never yields, and even while the
//! task is backing off before a resubscribe. A stream left alive would keep
//! the backend's subscription (a server cursor, a pooled connection) busy
//! for the life of the process.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use ciborium::Value as CborValue;
use futures::Stream;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, Dio, DioEvent, Lens};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, TableShell, Vista, VistaCapabilities, VistaChange, VistaChangeStream,
    VistaMetadata, forward_table_shell, mocks::MockShell,
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

/// How many of the streams a [`QuietShell`] hands out end at once; the rest
/// stay pending.
#[derive(Clone)]
enum Ends {
    All,
    First(Arc<AtomicUsize>),
}

impl Ends {
    fn next(&self) -> bool {
        match self {
            Ends::All => true,
            Ends::First(left) => left
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok(),
        }
    }
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
#[derive(Clone)]
struct QuietShell {
    inner: Box<dyn TableShell>,
    capabilities: VistaCapabilities,
    probe: Arc<Probe>,
    ends: Ends,
}

forward_table_shell!(QuietShell, inner, {
    async fn watch(&self) -> Result<VistaChangeStream> {
        self.probe.opened.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(QuietStream {
            probe: self.probe.clone(),
            end: self.ends.next(),
        }))
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
});

fn quiet_shell() -> MockShell {
    MockShell::new().with_metadata(
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_id_column("id"),
    )
}

async fn quiet_dio_over(inner: MockShell, ends: Ends) -> Result<(Dio, Arc<Probe>)> {
    let capabilities = inner.capabilities().with(Capability::Subscribe);
    let probe = Arc::new(Probe::default());
    let master = Vista::new("quiet", inner).map_shell(|inner| QuietShell {
        inner,
        capabilities,
        probe: probe.clone(),
        ends,
    });
    let lens = Lens::builder().cache(Cache::memory()).build()?;
    let dio = lens.dio(master).live().build().await?;
    Ok((dio, probe))
}

async fn quiet_dio(end: bool) -> Result<(Dio, Arc<Probe>)> {
    let ends = if end {
        Ends::All
    } else {
        Ends::First(Arc::new(AtomicUsize::new(0)))
    };
    quiet_dio_over(quiet_shell(), ends).await
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
    assert_eq!(probe.opened.load(Ordering::SeqCst), 1);

    // The follow is parked on a stream that will never yield.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(probe.dropped.load(Ordering::SeqCst), 0);

    drop(dio);
    assert!(
        eventually(Duration::from_millis(500), || {
            probe.dropped.load(Ordering::SeqCst) == 1
        })
        .await,
        "a change stream outlived its Dio"
    );
    Ok(())
}

#[tokio::test]
async fn a_dio_dropped_during_back_off_ends_without_waiting_it_out() -> Result<()> {
    let (dio, probe) = quiet_dio(true).await?;

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

/// The subscription drops and the master can't be listed when the follow
/// re-reads the set after resubscribing. The failure is published, and the
/// follow keeps retrying: once the master answers, a row written during the
/// gap reaches the cache.
#[tokio::test]
async fn a_failed_reread_after_a_resubscribe_is_published_and_retried() -> Result<()> {
    let shell = quiet_shell();
    shell.set_fail_reads(true);
    let (dio, probe) =
        quiet_dio_over(shell.clone(), Ends::First(Arc::new(AtomicUsize::new(1)))).await?;
    let mut bus = dio.subscribe_events();

    let failed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(DioEvent::RefreshFailed { error }) = bus.recv().await {
                return error;
            }
        }
    })
    .await
    .expect("the failed re-read is published");
    assert!(failed.contains("injected"), "{failed}");
    assert_eq!(probe.opened.load(Ordering::SeqCst), 2, "resubscribed once");

    let mut row = Record::new();
    row.insert("id".to_string(), CborValue::Text("gap".into()));
    shell.set_record("gap", row);
    shell.set_fail_reads(false);
    let facade = dio.cached();
    let recovered = tokio::time::timeout(Duration::from_secs(5), async {
        while facade.get_value("gap").await?.is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<_, vantage_core::VantageError>(())
    })
    .await;
    assert!(
        matches!(recovered, Ok(Ok(()))),
        "the follow retried the re-read"
    );
    Ok(())
}
