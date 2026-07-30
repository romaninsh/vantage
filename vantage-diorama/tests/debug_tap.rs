//! Integration tests for the official debug stream (`vantage_diorama::debug`).

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

use vantage_diorama::Lens;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

/// Collects every `vantage_diorama::debug` event as one flat string:
/// `"<message> ds=<..> <field>=<..> ..."` — order of fields as emitted.
struct CaptureLayer(Arc<Mutex<Vec<String>>>);

struct FlatVisitor {
    message: String,
    fields: String,
}

impl Visit for FlatVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != "vantage_diorama::debug" {
            return;
        }
        let mut v = FlatVisitor {
            message: String::new(),
            fields: String::new(),
        };
        event.record(&mut v);
        self.0
            .lock()
            .unwrap()
            .push(format!("{}{}", v.message, v.fields));
    }
}

/// Install a thread-local subscriber that captures every
/// `vantage_diorama::debug` event as a flat string. Returns the guard
/// (drop it to uninstall) and the shared log the events land in.
///
/// Unused by this task's own test (which only checks the tap reaches the
/// Dio, off means off) — later tasks in this stream add events and use
/// this to assert on the emitted lines.
#[allow(dead_code)]
pub fn capture() -> (tracing::subscriber::DefaultGuard, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer(log.clone()));
    (tracing::subscriber::set_default(subscriber), log)
}

/// Captured lines whose flattened text contains `needle`.
#[allow(dead_code)]
pub fn lines_containing(log: &Arc<Mutex<Vec<String>>>, needle: &str) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains(needle))
        .cloned()
        .collect()
}

/// Minimal master Vista — an `id` column only, no rows. These tests only
/// exercise the tap plumbing, not data flow.
fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    Vista::new("books", Box::new(MockShell::new().with_metadata(metadata)))
}

#[tokio::test]
async fn builder_flag_reaches_the_dio_and_off_means_off() {
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .debug_datasource("faker-ds")
            .runtime(tokio::runtime::Handle::current())
            .build()
            .unwrap(),
    );
    let dio = lens.make_dio(master()).await.unwrap();
    assert!(dio.debug_tap().enabled());
    assert_eq!(dio.debug_tap().ds(), "faker-ds");

    let quiet = Arc::new(
        Lens::new()
            .cache_in_memory()
            .runtime(tokio::runtime::Handle::current())
            .build()
            .unwrap(),
    );
    let dio = quiet.make_dio(master()).await.unwrap();
    assert!(!dio.debug_tap().enabled());
}
