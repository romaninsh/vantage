//! Advertised read capabilities must be backed by a working method.

use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;

use super::Probe;
use super::narrowing::narrow_into;
use crate::Vista;

/// For each advertised capability, call its method once.
///
/// Reads (`fetch_window`, `fetch_page`, `fetch_next`, `get_count`, `watch`)
/// must succeed. Traversal (`get_ref`, `get_ref_target`) only must not be
/// refused. The capabilities that mutate the vista (`can_set_page_size`,
/// `can_search`, `can_order`, `can_filter_operators`) run on clones of the
/// shell.
///
/// When the shell can't be cloned, the narrowing probes are skipped (with a
/// note on stderr), and the page reads only must not be refused, since no
/// page size can be set. [`check_narrowing`](super::check_narrowing) covers
/// both on a throwaway vista.
pub async fn check_advertised(vista: &Vista) {
    let caps = vista.capabilities().clone();
    let mut probe = Probe::default();

    if caps.can_fetch_window {
        probe.check_ok(
            "can_fetch_window",
            "fetch_window",
            &vista.fetch_window(0, 1).await,
        );
    }
    if caps.can_count {
        probe.check_ok("can_count", "get_count", &vista.get_count().await);
    }
    if caps.can_subscribe {
        // Opening the subscription is the probe; the stream is dropped unread.
        probe.check_ok("can_subscribe", "watch", &vista.watch().await.map(drop));
    }
    if let Some(relation) = vista.get_references().into_iter().next() {
        if caps.can_traverse_to_record {
            let row = match vista.get_some_value().await {
                Ok(Some((_, row))) => row,
                _ => Record::new(),
            };
            probe.check(
                "can_traverse_to_record",
                "get_ref",
                &vista.get_ref(&relation, &row).map(drop),
            );
        }
        if caps.can_traverse_to_set {
            probe.check(
                "can_traverse_to_set",
                "get_ref_target",
                &vista.get_ref_target(&relation).map(drop),
            );
        }
    }

    match vista.source.clone_shell() {
        Some(shell) => {
            // Paging and narrowing each get their own clone, so the page
            // size doesn't leak into the narrowing reads and vice versa.
            let mut paged = Vista::new(vista.name(), shell);
            set_page_size(&mut paged, &mut probe);
            probe_pages(&paged, &mut probe, true).await;
            if let Some(shell) = vista.source.clone_shell() {
                narrow_into(&mut Vista::new(vista.name(), shell), &mut probe).await;
            }
        }
        None => {
            if caps.can_set_page_size || narrows(vista) {
                eprintln!(
                    "vista contract: `{}` can't be cloned, so check_advertised skips the \
                     page-size and narrowing probes; run check_narrowing on a throwaway vista",
                    vista.name()
                );
            }
            probe_pages(vista, &mut probe, !caps.can_set_page_size).await;
        }
    }
    probe.finish();
}

/// `set_page_size(1)` when advertised.
pub(super) fn set_page_size(vista: &mut Vista, probe: &mut Probe) {
    if vista.capabilities().can_set_page_size {
        let result = vista.set_page_size(1);
        probe.check_ok("can_set_page_size", "set_page_size", &result);
    }
}

/// `fetch_page(1)` and `fetch_next(None)` when advertised. `strict` holds
/// them to success; otherwise they only must not be refused.
pub(super) async fn probe_pages(vista: &Vista, probe: &mut Probe, strict: bool) {
    let caps = vista.capabilities().clone();
    if caps.can_fetch_page {
        let result = vista.fetch_page(1).await;
        if strict {
            probe.check_ok("can_fetch_page", "fetch_page", &result);
        } else {
            probe.check("can_fetch_page", "fetch_page", &result);
        }
    }
    if caps.can_fetch_next {
        let result = vista.fetch_next(None).await;
        if strict {
            probe.check_ok("can_fetch_next", "fetch_next", &result);
        } else {
            probe.check("can_fetch_next", "fetch_next", &result);
        }
    }
}

fn narrows(vista: &Vista) -> bool {
    let caps = vista.capabilities();
    caps.can_search || caps.can_order || caps.can_filter_operators
}
