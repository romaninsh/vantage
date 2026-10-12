//! Whether an accepted narrowing clause changed what the vista reads. Each
//! probe runs only on a fixture that lists rows.

use vantage_dataset::traits::ReadableValueSet;

use super::{Rows, UNMATCHABLE};
use crate::{Capability, SortDirection, Vista, contract::Probe};

/// A search for a needle no row holds must count fewer rows than the
/// un-searched set.
pub(super) async fn search(vista: &Vista, searched: &Vista, probe: &mut Probe) {
    match (vista.count().await, searched.count().await) {
        (Ok(all), Ok(found)) if found < all => {}
        (Ok(all), Ok(found)) => probe.fail(format!(
            "`{}` is advertised but with_search({UNMATCHABLE:?}) counts {found} of {all} rows: \
             the search is not applied",
            Capability::Search
        )),
        (Err(e), _) | (_, Err(e)) => probe.fail(format!(
            "`{}` is advertised but count fails around with_search: {e}",
            Capability::Search
        )),
    }
}

/// `column > pivot` must list a subset of `all` that leaves out the pivot
/// row itself.
pub(super) async fn filter_gt(
    filtered: &Vista,
    column: &str,
    all: &Rows,
    pivot_id: &str,
    probe: &mut Probe,
) {
    let Ok(rows) = filtered.list_values().await else {
        return;
    };
    let capability = Capability::FilterOps;
    if rows.contains_key(pivot_id) {
        probe.fail(format!(
            "`{capability}` is advertised but with_condition({column} > <row {pivot_id}>) \
             still lists row {pivot_id}: the condition is not applied"
        ));
    }
    if let Some(stray) = rows.keys().find(|id| !all.contains_key(*id)) {
        probe.fail(format!(
            "`{capability}` is advertised but with_condition({column} > ..) lists row {stray}, \
             which the un-narrowed set does not hold"
        ));
    }
}

/// Ascending and descending order on `column` must list different first
/// rows. Skipped when fewer than two rows exist, or every row holds the same
/// value in `column`, since then both orders may agree.
pub(super) async fn order(vista: &Vista, asc: &Vista, column: &str, all: &Rows, probe: &mut Probe) {
    if !distinct_values(vista, column, all) {
        return;
    }
    let capability = Capability::Order;
    let desc = match vista.clone().with_order(column, SortDirection::Descending) {
        Ok(desc) => desc,
        Err(e) => {
            probe.check_ok(capability, "with_order(desc)", &Err::<(), _>(e));
            return;
        }
    };
    let (asc_rows, desc_rows) = match (asc.list_values().await, desc.list_values().await) {
        (Ok(a), Ok(d)) => (a, d),
        (Err(e), _) | (_, Err(e)) => {
            probe.check_ok(capability, "list_values after with_order", &Err::<(), _>(e));
            return;
        }
    };
    let first_asc = asc_rows.keys().next();
    if first_asc == desc_rows.keys().next() {
        probe.fail(format!(
            "`{capability}` is advertised but ordering by {column} ascending and descending \
             both list row {} first: the order is not applied",
            first_asc.map(String::as_str).unwrap_or("<none>")
        ));
    }
}

/// Whether `all` holds at least two rows with different values in `column`.
/// The id column always does, since ids are distinct even when rows don't
/// carry them as a cell.
fn distinct_values(vista: &Vista, column: &str, all: &Rows) -> bool {
    if all.len() < 2 {
        return false;
    }
    if vista.id_column() == Some(column) {
        return true;
    }
    let mut values = all.values().map(|row| row.get(column));
    let first = values.next().flatten();
    values.any(|v| v != first)
}
