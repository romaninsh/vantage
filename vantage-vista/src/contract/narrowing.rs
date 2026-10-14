//! Advertised narrowing capabilities must be backed by a working method
//! that takes effect.

use indexmap::IndexMap;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;

use super::Probe;
use crate::{Capability, CborValue, ColumnFlag, FilterOp, NativeCondition, SortDirection, Vista};

mod dotted;
mod effect;

/// For each advertised narrowing capability, apply its clause to a clone of
/// `vista` with `with_*` and read through the result, which must succeed.
/// When `vista` lists rows, each clause must also take effect:
/// - `Search` with a needle no row holds counts fewer rows;
/// - `FilterOps`: `id > <a row's id>` lists a subset without that row;
/// - `Order`: ascending and descending list different first rows, unless
///   every row holds the same value in the order column.
///
/// `NativeConditions` gets a condition of a type no driver takes, so it only
/// must not be refused.
///
/// Every vista also gets an equality on a dotted path (`root.leaf`) into a
/// declared column, which the shell may refuse as `Unimplemented`
/// ([`condition_column_error`](crate::condition_column_error)); a plain error fails. The effect of an accepted
/// path is checked only when a fixture row holds a map in a declared column:
/// the condition built from that map must list the row, and leave out a row
/// whose map lacks the entry. Without such a row, an accepted path must only
/// read. `vista` itself is left as it was.
pub async fn check_narrowing(vista: &Vista) {
    let mut probe = Probe::default();
    narrow_into(vista, &mut probe).await;
    probe.finish();
}

/// A condition type no driver takes, for the `NativeConditions` probe.
struct ProbeCondition;

/// A search needle no fixture row holds.
const UNMATCHABLE: &str = "3f9c2a7e-0b4d-4e61-9a85-d17c5be04f2a";

type Rows = IndexMap<String, Record<CborValue>>;

/// The search, order, operator-filter, native-condition and dotted-path
/// probes, recorded into `probe`.
pub(super) async fn narrow_into(vista: &Vista, probe: &mut Probe) {
    let caps = *vista.capabilities();
    // The un-narrowed rows the effect probes compare against. A fixture
    // that can't list, or lists nothing, is held to "not refused" only.
    let listed: Option<Rows> = vista.list_values().await.ok();
    let all = listed.as_ref().filter(|r| !r.is_empty());

    if caps.has(Capability::Search) {
        let searched = vista.clone().with_search(UNMATCHABLE);
        if let Some(searched) =
            read_through(probe, Capability::Search, "with_search", searched).await
            && all.is_some()
        {
            effect::search(vista, &searched, probe).await;
        }
    }
    if caps.has(Capability::Order) {
        match order_column(vista) {
            Some(column) => {
                let ordered = vista.clone().with_order(&column, SortDirection::Ascending);
                if let Some(asc) =
                    read_through(probe, Capability::Order, "with_order", ordered).await
                    && let Some(all) = all
                {
                    effect::order(vista, &asc, &column, all, probe).await;
                }
            }
            None => probe
                .fail("`order` is advertised but the vista has no column to order by".to_string()),
        }
    }
    if caps.has(Capability::FilterOps) {
        filter_ops(vista, all, probe).await;
    }
    dotted::dotted_path(vista, listed.as_ref(), probe).await;
    if caps.has(Capability::NativeConditions) {
        let result = vista
            .clone()
            .with_native_condition(NativeCondition::new(ProbeCondition, "contract-probe"));
        probe.check(
            Capability::NativeConditions,
            "with_native_condition",
            &result.map(drop),
        );
    }
}

/// `Gt` on the id column (or the first column) against a value of that
/// column's own type, taken from a row when the set has one.
async fn filter_ops(vista: &Vista, all: Option<&Rows>, probe: &mut Probe) {
    let column = vista
        .id_column()
        .or_else(|| vista.columns().keys().next().map(String::as_str))
        .map(str::to_string);
    let Some(column) = column else {
        probe.fail("`filter_ops` is advertised but the vista has no columns".to_string());
        return;
    };
    // The first row carrying the column, so the operator compares like with
    // like and the row itself must drop out.
    let pivot = all.and_then(|rows| {
        rows.iter()
            .find_map(|(id, row)| row.get(&column).map(|v| (id.as_str(), v.clone())))
    });
    let (pivot_id, value) = match pivot {
        Some((id, value)) => (Some(id), value),
        None => (None, CborValue::Text(String::new())),
    };
    let filtered = vista.clone().with_condition(&column, FilterOp::Gt, value);
    let method = "with_condition(Gt)";
    if let Some(filtered) = read_through(probe, Capability::FilterOps, method, filtered).await
        && let (Some(all), Some(pivot_id)) = (all, pivot_id)
    {
        effect::filter_gt(&filtered, &column, all, pivot_id, probe).await;
    }
}

/// Record whether `narrowed` was built and lists; the narrowed vista when
/// both held.
async fn read_through(
    probe: &mut Probe,
    capability: Capability,
    method: &str,
    narrowed: vantage_core::Result<Vista>,
) -> Option<Vista> {
    match narrowed {
        Ok(narrowed) => {
            let listed = narrowed.list_values().await;
            let ok = listed.is_ok();
            probe.check_ok(capability, &format!("list_values after {method}"), &listed);
            ok.then_some(narrowed)
        }
        Err(e) => {
            probe.check_ok(capability, method, &Err::<(), _>(e));
            None
        }
    }
}

/// The id column when it is orderable, else the first orderable column.
/// Falls back to the id column, so a vista advertising `Order` with no
/// orderable column fails the probe rather than skipping it.
fn order_column(vista: &Vista) -> Option<String> {
    let orderable = |name: &str| {
        vista
            .column(name)
            .is_some_and(|c| c.has_flag(ColumnFlag::Orderable))
    };
    vista
        .id_column()
        .filter(|id| orderable(id))
        .or_else(|| {
            vista
                .columns()
                .keys()
                .map(String::as_str)
                .find(|c| orderable(c))
        })
        .or(vista.id_column())
        .map(str::to_string)
}
