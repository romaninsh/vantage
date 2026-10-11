//! Advertised narrowing capabilities must be backed by a working method.

use vantage_dataset::traits::ReadableValueSet;

use super::Probe;
use super::advertised::{probe_pages, set_page_size};
use crate::{CborValue, FilterOp, SortDirection, Vista, flags};

/// For each advertised capability that mutates the vista, call its method
/// and read through the result, which must succeed. After a successful
/// `set_page_size`, `fetch_page` and `fetch_next` must succeed too.
///
/// `vista` ends up narrowed; pass a clone or a fresh fixture.
pub async fn check_narrowing(vista: &mut Vista) {
    let mut probe = Probe::default();
    if vista.capabilities().can_set_page_size {
        set_page_size(vista, &mut probe);
        probe_pages(vista, &mut probe, true).await;
    }
    narrow_into(vista, &mut probe).await;
    probe.finish();
}

/// The search, order and operator-filter probes, recorded into `probe`.
pub(super) async fn narrow_into(vista: &mut Vista, probe: &mut Probe) {
    let caps = vista.capabilities().clone();
    // A value of the id column's own type, so the operator probe compares
    // like with like.
    let id_value = match (vista.get_id_column(), vista.get_some_value().await) {
        (Some(id), Ok(Some((_, row)))) => row.get(id).cloned(),
        _ => None,
    };

    if caps.can_search {
        probe.check_ok("can_search", "add_search", &vista.add_search("a"));
        probe.check_ok(
            "can_search",
            "list_values after add_search",
            &vista.list_values().await,
        );
    }
    if caps.can_order {
        match order_column(vista) {
            Some(column) => {
                probe.check_ok(
                    "can_order",
                    "add_order",
                    &vista.add_order(&column, SortDirection::Ascending),
                );
                probe.check_ok(
                    "can_order",
                    "list_values after add_order",
                    &vista.list_values().await,
                );
            }
            None => probe.fail(
                "`can_order` is advertised but the vista has no column to order by".to_string(),
            ),
        }
    }
    if caps.can_filter_operators {
        let column = vista
            .get_id_column()
            .or_else(|| vista.get_column_names().into_iter().next())
            .map(str::to_string);
        match column {
            Some(column) => {
                let value = id_value.unwrap_or_else(|| CborValue::Text(String::new()));
                probe.check_ok(
                    "can_filter_operators",
                    "add_condition(Gt)",
                    &vista.add_condition(column, FilterOp::Gt, value),
                );
                probe.check_ok(
                    "can_filter_operators",
                    "list_values after add_condition(Gt)",
                    &vista.list_values().await,
                );
            }
            None => probe.fail(
                "`can_filter_operators` is advertised but the vista has no columns".to_string(),
            ),
        }
    }
}

/// The id column when it is orderable, else the first orderable column.
/// Falls back to the id column, so a vista advertising `can_order` with no
/// orderable column fails the probe rather than skipping it.
fn order_column(vista: &Vista) -> Option<String> {
    let orderable = |name: &str| {
        vista
            .get_column(name)
            .is_some_and(|c| c.has_flag(flags::ORDERABLE))
    };
    vista
        .get_id_column()
        .filter(|id| orderable(id))
        .or_else(|| vista.get_column_names().into_iter().find(|c| orderable(c)))
        .or(vista.get_id_column())
        .map(str::to_string)
}
