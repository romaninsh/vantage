//! The leading window is the floor: every shell that lists answers
//! `fetch_window(0, n)`, whether or not it advertises `Window`.

use vantage_dataset::traits::ReadableValueSet;

use super::Probe;
use crate::{CborValue, Page, Vista};

/// When `list_values` succeeds on the fixture, `fetch_window(0, n)` must
/// succeed and return `min(n, listed)` rows of the set. When an order is
/// applied (`vista.query().order`), they must be the first in that order:
/// the order column's values match those of the first listed rows, so ties
/// may come back in any order. Without an order, a shell's list and window
/// may order rows differently. Probed with `n = 1`, one short of the
/// listing (catching a shell that ignores the limit) and one past it. A
/// fixture that can't list is skipped.
///
/// [`check_advertised`](super::check_advertised) runs this on every vista.
pub(super) async fn leading_into(vista: &Vista, probe: &mut Probe) {
    let Ok(listed) = vista.list_values().await else {
        return;
    };
    let mut limits = vec![1, listed.len().saturating_sub(1).max(1), listed.len() + 1];
    limits.dedup();
    for limit in limits {
        let page = match vista.fetch_window(0, limit).await {
            Ok(page) => page,
            Err(e) => {
                probe.fail(format!(
                    "`list` works but fetch_window(0, {limit}) fails: {e}; every shell that \
                     lists answers the leading window"
                ));
                continue;
            }
        };
        let expected = limit.min(listed.len());
        if page.rows.len() > limit {
            probe.fail(format!(
                "fetch_window(0, {limit}) returns {} rows: the limit is not applied",
                page.rows.len()
            ));
            continue;
        }
        if page.rows.len() != expected {
            probe.fail(format!(
                "fetch_window(0, {limit}) returns {} rows of a set listing {}; it must \
                 return {expected}",
                page.rows.len(),
                listed.len()
            ));
            continue;
        }
        if let Some((id, _)) = page.rows.iter().find(|(id, _)| !listed.contains_key(id)) {
            probe.fail(format!(
                "fetch_window(0, {limit}) returns row {id}, which the set doesn't list"
            ));
            continue;
        }
        if let Some((column, _)) = &vista.query().order {
            let want: Vec<Option<&CborValue>> = listed
                .values()
                .take(expected)
                .map(|r| r.get(column))
                .collect();
            let got = column_values(&page, column);
            if got != want {
                probe.fail(format!(
                    "ordered by `{column}`, fetch_window(0, {limit}) returns {got:?}, not the \
                     first values in the set's order {want:?}"
                ));
            }
        }
    }
}

fn column_values<'a>(page: &'a Page, column: &str) -> Vec<Option<&'a CborValue>> {
    page.rows.iter().map(|(_, r)| r.get(column)).collect()
}
