//! Capturing the official debug stream (`vantage_diorama::debug`).

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt};

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
pub fn capture() -> (tracing::subscriber::DefaultGuard, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer(log.clone()));
    (tracing::subscriber::set_default(subscriber), log)
}

/// Captured lines whose flattened text contains `needle`.
pub fn lines_containing(log: &Arc<Mutex<Vec<String>>>, needle: &str) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains(needle))
        .cloned()
        .collect()
}
