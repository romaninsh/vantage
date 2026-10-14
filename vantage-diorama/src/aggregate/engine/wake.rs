//! What wakes the recomputation loop.

use tokio::sync::{Notify, broadcast};

use crate::DioEvent;
use crate::debug::DebugTap;

pub(super) enum Wake {
    /// The trigger label for the recompute this wake causes — `Some` only
    /// when the tap is enabled, since it exists purely for the debug line.
    ///
    /// Two spellings meet in this label: a real `DioEvent` variant comes
    /// through Debug-formatted and PascalCase (`"RecordChanged"`); a wake
    /// with no event behind it — the local write-through nudge, or a lagged
    /// bus counted as "something changed" — is spelled lowercase
    /// (`"nudge"`, `"lagged"`), the same register as `"initial"` and
    /// `"debounce-trailing"` at the call sites in `run`.
    Changed(Option<String>),
    SourceGone,
}

/// The event's variant name, Debug-formatted and trimmed of its fields —
/// `RecordChanged { id: "5" }` becomes `"RecordChanged"`. Cheap enough to
/// build only where it is: on the wake path, gated behind `tap.enabled()`.
fn event_trigger(event: &DioEvent) -> String {
    let debug = format!("{event:?}");
    debug.split([' ', '(']).next().unwrap_or(&debug).to_string()
}

/// Block until the source's rows may have changed.
///
/// The filter is otherwise deliberately permissive: over-triggering costs one
/// recomputation whose output comparison then suppresses the repaint, whereas
/// under-triggering leaves the layer silently wrong. A `Lagged` bus — events
/// dropped because this task fell behind — is simply another reason to
/// recompute, which is the whole benefit of holding no incremental state.
///
/// Two kinds of event are excluded, for different reasons.
///
/// **Loading events** (`ViewportChanged`, `LoadFailed`, `Hydrating`) describe a
/// viewport moving or a pass making progress, not rows changing. Two-pass
/// hydration emits `RecordChanged` for every row it fills in, so nothing is
/// missed by ignoring them.
///
/// `RangeLoaded` is NOT among them, though it reads like one. A single-pass
/// chunk load writes its rows through [`crate::ChunkSink::push`],
/// straight into the
/// cache — it emits no per-row event, and `RangeLoaded` is the only thing it
/// announces. Ignoring it meant a paged source could load its whole first page
/// and every aggregate over it would still be reporting the empty set it was
/// derived over: a dashboard opening on a cold cache showed three zeros and
/// kept showing them. It is also what makes an aggregate *progressive* — each
/// page that lands recomputes over everything held so far.
///
/// **`Refreshing`** announces that a refresh has *started*. It is emitted
/// before the source's `on_refresh` runs, and the common shape of that callback
/// is "clear the cache, then refill it" — so reacting here would read the
/// source mid-clear and publish a zero that the trailing `DatasetChanged`
/// immediately corrects. Waiting for that trailing event is what keeps a
/// refresh from flickering every derived value through empty. If the refresh
/// fails there is no `DatasetChanged` and the layer keeps its last good output,
/// which is the same stale-over-blank choice made everywhere else here; the
/// `RefreshFailed` a failed live re-read publishes is skipped for that reason.
pub(super) async fn wait(
    bus: &mut broadcast::Receiver<DioEvent>,
    nudge: &Notify,
    tap: &DebugTap,
) -> Wake {
    loop {
        tokio::select! {
            _ = nudge.notified() => {
                return Wake::Changed(tap.enabled().then(|| "nudge".to_string()));
            }
            received = bus.recv() => match received {
                Ok(DioEvent::ViewportChanged { .. })
                | Ok(DioEvent::LoadFailed { .. })
                | Ok(DioEvent::RefreshFailed { .. })
                | Ok(DioEvent::Hydrating { .. })
                | Ok(DioEvent::Refreshing) => continue,
                Ok(event) => {
                    return Wake::Changed(tap.enabled().then(|| event_trigger(&event)));
                }
                Err(broadcast::error::RecvError::Lagged(dropped)) => {
                    tracing::debug!(
                        dropped,
                        "source event bus lagged — recomputing from the current rows",
                    );
                    // "lagged" is intentionally untested: forcing a broadcast
                    // receiver to fall behind deterministically (fill its
                    // ring buffer faster than this task drains it) isn't
                    // worth the test complexity for one trigger label. The
                    // label exists so a real lag shows up in the stream
                    // rather than masquerading as some other trigger.
                    return Wake::Changed(tap.enabled().then(|| "lagged".to_string()));
                }
                Err(broadcast::error::RecvError::Closed) => return Wake::SourceGone,
            },
        }
    }
}
