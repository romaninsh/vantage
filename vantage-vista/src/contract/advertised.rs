//! Advertised capabilities must be backed by a working method.

use vantage_dataset::traits::ReadableValueSet;

use super::Probe;
use super::narrowing::narrow_into;
use super::window::leading_into;
use crate::{Capability, Vista};

/// For each capability the vista advertises, call its method once.
///
/// - The leading window, advertised `Window` or not: `fetch_window(0, n)`
///   returns `min(n, listed)` rows of the set, the first in the set's order
///   when an order is applied.
/// - `Window`, `Cursor`, `Subscribe`: `fetch_window(1, 1)` (an offset past
///   the leading window), `fetch_cursor(None, 1)` and `watch()` must
///   succeed.
/// - `Search`, `Order`, `FilterOps`, `NativeConditions`: probed on clones,
///   see [`check_narrowing`](super::check_narrowing).
/// - `Traverse`: `get_ref` on every reference, joined or contained, with a
///   row of the set and with none, must not be refused; a join with keys
///   traversed from a row must come back narrowed (`is_narrowed()`).
/// - `Insert`, `Update`, `Delete`, `DeleteAll`, `Import`, `ConfineWrites`: left to
///   [`check_advertised_writes`](super::check_advertised_writes), which
///   needs a disposable fixture.
pub async fn check_advertised(vista: &Vista) {
    let mut probe = Probe::default();
    for capability in vista.capabilities().iter() {
        match capability {
            Capability::Window => probe.check_ok(
                capability,
                "fetch_window(1, 1)",
                &vista.fetch_window(1, 1).await,
            ),
            Capability::Cursor => probe.check_ok(
                capability,
                "fetch_cursor",
                &vista.fetch_cursor(None, 1).await,
            ),
            // Opening the subscription is the probe; the stream is dropped unread.
            Capability::Subscribe => {
                probe.check_ok(capability, "watch", &vista.watch().await.map(drop))
            }
            Capability::Traverse => probe_traverse(vista, &mut probe).await,
            Capability::Search
            | Capability::Order
            | Capability::FilterOps
            | Capability::NativeConditions
            | Capability::Insert
            | Capability::Update
            | Capability::Delete
            | Capability::DeleteAll
            | Capability::Import
            | Capability::ConfineWrites
            | Capability::WritesReachHooks => {}
        }
    }
    leading_into(vista, &mut probe).await;
    narrow_into(vista, &mut probe).await;
    probe.finish();
}

/// Every reference, joined or contained, with a row of the set and with
/// none. A join with keys traversed from a real row must come back
/// narrowed, recorded in the target's `query()`.
async fn probe_traverse(vista: &Vista, probe: &mut Probe) {
    let row = match vista.get_some_value().await {
        Ok(Some((_, row))) => Some(row),
        _ => None,
    };
    for reference in vista.references().values() {
        let relation = reference.name.as_str();
        let from_row = vista.get_ref(relation, Some(&row.clone().unwrap_or_default()));
        probe.check(
            Capability::Traverse,
            &format!("get_ref({relation}, Some(row))"),
            &from_row,
        );
        if let (Ok(target), Some(_)) = (&from_row, &row)
            && !reference.kind.is_contained()
            && !reference.keys.is_empty()
            && !target.is_narrowed()
        {
            probe.fail(format!(
                "`traverse`: get_ref({relation}, Some(row)) returned a target whose query() \
                 records no narrowing; narrow it through Vista::add_condition"
            ));
        }
        probe.check(
            Capability::Traverse,
            &format!("get_ref({relation}, None)"),
            &vista.get_ref(relation, None).map(drop),
        );
    }
}
