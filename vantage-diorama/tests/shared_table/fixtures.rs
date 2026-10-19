//! The invoice table every test here shares, two Dios over it, and the
//! readers the tests assert through.
//!
//! The pair is the shape of the bug: A over the whole table, and B derived
//! from it for client 7. Each has its own cache table and its own bus, so
//! before the table hub, a change settled through A never reached B.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use futures::channel::mpsc;
use tokio::sync::broadcast;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, ChangeFlash, Dio, DioEvent, Lens, TableScenery};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Capability, Condition, FilterOp, TableShell, Vista, VistaCapabilities, VistaChange,
    VistaChangeStream, forward_table_shell,
};

use crate::support::{
    augmented_dio, copy_master, fixed_augmentation, memory_lens, meta, quick_lens, record,
};

/// Bounds every wait here. A hang fails the test; it is not a speed bound.
pub const WITHIN: Duration = Duration::from_secs(5);

/// Rows `x` (client 7, amount 10) and `z` (client 8, amount 5).
pub fn invoice_master() -> MockShell {
    MockShell::new()
        .with_metadata(meta(&["id", "client_id", "amount", "note"]))
        .with_record("x", invoice("x", "7", "10"))
        .with_record("z", invoice("z", "8", "5"))
}

pub fn invoice(id: &str, client: &str, amount: &str) -> Record<CborValue> {
    record(&[("id", id), ("client_id", client), ("amount", amount)])
}

pub fn client(id: &str) -> Condition {
    Condition::new("client_id", FilterOp::Eq, id)
}

/// A patch of row `id` setting `pairs`.
pub fn patch(id: &str, pairs: &[(&str, &str)]) -> ChangeFlash {
    ChangeFlash::partial(id, record(pairs))
}

/// The detail source the two-pass pair merges `note` from.
pub fn note_detail() -> Vista {
    let shell = ["x", "y", "z"]
        .into_iter()
        .fold(
            MockShell::new().with_metadata(meta(&["id", "note"])),
            |shell, id| shell.with_record(id, record(&[("id", id), ("note", &format!("n-{id}"))])),
        )
        .get_only();
    Vista::new("notes", shell)
}

/// A over the whole invoice table, B derived from it for client 7, on one
/// Lens, and the shell behind both.
pub struct Pair {
    pub a: Dio,
    pub b: Dio,
    pub shell: MockShell,
}

impl Pair {
    /// Both copied whole into an in-memory cache by an eager start.
    pub async fn eager() -> Self {
        Self::on(&memory_lens(copy_master()), invoice_master()).await
    }

    /// A and B over `shell`, on `lens`.
    pub async fn on(lens: &Arc<Lens>, shell: MockShell) -> Self {
        let a = lens
            .dio(Vista::new("invoice", shell.clone()))
            .build()
            .await
            .expect("A builds");
        Self::with_a(a, shell).await
    }

    /// Both two-pass: A lists the master and merges each row's `note` from
    /// [`note_detail`], and B shares A's augmentations.
    pub async fn two_pass() -> Self {
        let shell = invoice_master();
        let a = augmented_dio(
            &quick_lens(Cache::memory()),
            Vista::new("invoice", shell.clone()),
            vec![fixed_augmentation(note_detail(), &["note"])],
        )
        .await;
        Self::with_a(a, shell).await
    }

    async fn with_a(a: Dio, shell: MockShell) -> Self {
        let b = a
            .derive()
            .with_condition(client("7"))
            .build()
            .await
            .expect("B builds");
        Self { a, b, shell }
    }
}

/// The ids a view shows, in its order.
pub fn ids(view: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..view.row_count())
        .filter_map(|i| view.row(i))
        .filter_map(|row| text_of(row.record.get("id")))
        .collect()
}

/// Column `col` of row `id` as the view shows it.
pub fn cell(view: &Arc<dyn TableScenery>, id: &str, col: &str) -> Option<String> {
    view.row_by_id(id)
        .and_then(|row| text_of(row.record.get(col)))
}

fn text_of(value: Option<&CborValue>) -> Option<String> {
    match value {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Column `col` of the row `dio` caches under `id`.
pub async fn cached_cell(dio: &Dio, id: &str, col: &str) -> Option<String> {
    let row = dio.cached().get_value(id).await.ok().flatten()?;
    text_of(row.get(col))
}

pub async fn holds(dio: &Dio, id: &str) -> bool {
    dio.cached().get_value(id).await.ok().flatten().is_some()
}

/// Open `dio`'s single-pass view and wait until it shows `want`.
pub async fn view_showing(dio: &Dio, want: &[&str]) -> Arc<dyn TableScenery> {
    let view = dio.table_scenery().open().await.expect("view opens");
    until("the view shows its first rows", async || ids(&view) == want).await;
    view
}

/// Poll `holds` until it is true, or panic naming `what`.
pub async fn until(what: &str, holds: impl AsyncFn() -> bool) {
    let wait = async {
        while !holds().await {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    if tokio::time::timeout(WITHIN, wait).await.is_err() {
        panic!("{what}: not within {WITHIN:?}");
    }
}

/// The per-record events `bus` has waiting, labelled `"<kind> <id>"`, drained.
pub fn settle_events(bus: &mut broadcast::Receiver<DioEvent>) -> Vec<String> {
    std::iter::from_fn(|| bus.try_recv().ok())
        .filter_map(label)
        .collect()
}

fn label(event: DioEvent) -> Option<String> {
    let (kind, id) = match event {
        DioEvent::WritePending { id, .. } => ("pending", id),
        DioEvent::RecordChanged { id } => ("changed", id),
        DioEvent::RecordInserted { id } => ("inserted", id),
        DioEvent::RecordRemoved { id } => ("removed", id),
        DioEvent::WriteReverted { id, .. } => ("reverted", id),
        _ => return None,
    };
    Some(format!("{kind} {id}"))
}

/// Wait until `bus` announces `want` (as [`settle_events`] labels it), and
/// return every per-record event up to and including it. The sentinel
/// behind every "never arrives" assertion: a fact published before the
/// sentinel's is applied before it.
pub async fn events_until(bus: &mut broadcast::Receiver<DioEvent>, want: &str) -> Vec<String> {
    let mut seen = Vec::new();
    let wait = async {
        loop {
            let Ok(event) = bus.recv().await else {
                continue;
            };
            if let Some(event) = label(event) {
                let done = event == want;
                seen.push(event);
                if done {
                    return;
                }
            }
        }
    };
    if tokio::time::timeout(WITHIN, wait).await.is_err() {
        panic!("never saw {want:?}; saw {seen:?}");
    }
    seen
}

// ---- a master that pushes changes ------------------------------------------

/// The change streams a [`live_master`] hands out, and the store behind it.
#[derive(Clone)]
pub struct LiveFeed {
    pub store: MockShell,
    streams: Arc<Mutex<Vec<mpsc::UnboundedSender<Result<VistaChange>>>>>,
    opened: Arc<AtomicUsize>,
}

impl LiveFeed {
    /// The backend changes: the store takes `change`, and every open stream
    /// pushes it.
    pub fn push(&self, change: VistaChange) {
        match &change {
            VistaChange::Inserted { id, value } | VistaChange::Updated { id, value } => {
                self.store.set_record(id.clone(), value.clone())
            }
            VistaChange::Deleted { id } => self.store.remove_record(id),
            _ => {}
        }
        let mut streams = self.streams.lock().unwrap();
        streams.retain(|tx| tx.unbounded_send(Ok(change.clone())).is_ok());
    }

    /// End every open stream, as a dropped connection does.
    pub fn end_streams(&self) {
        self.streams.lock().unwrap().clear();
    }

    /// How many subscriptions the master has handed out.
    pub fn subscriptions(&self) -> usize {
        self.opened.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
struct LiveShell {
    inner: Box<dyn TableShell>,
    capabilities: VistaCapabilities,
    feed: LiveFeed,
}

forward_table_shell!(LiveShell, inner, {
    async fn watch(&self) -> Result<VistaChangeStream> {
        self.feed.opened.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::unbounded();
        self.feed.streams.lock().unwrap().push(tx);
        Ok(Box::pin(rx))
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
});

/// The invoice table over a master advertising `Subscribe`, and its feed.
pub fn live_master() -> (Vista, LiveFeed) {
    let store = invoice_master();
    let capabilities = store.capabilities().with(Capability::Subscribe);
    let feed = LiveFeed {
        store: store.clone(),
        streams: Arc::default(),
        opened: Arc::default(),
    };
    let shell = feed.clone();
    let vista = Vista::new("invoice", store).map_shell(|inner| LiveShell {
        inner,
        capabilities,
        feed: shell,
    });
    (vista, feed)
}

pub fn updated(id: &str, client: &str, amount: &str) -> VistaChange {
    VistaChange::Updated {
        id: id.into(),
        value: invoice(id, client, amount),
    }
}
