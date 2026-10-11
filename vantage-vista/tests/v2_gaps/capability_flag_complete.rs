//! `TableShell::capability_flag` must answer every `VistaCapabilities` field.
//!
//! Today the lookup is a hand-kept string match that has drifted: it has no
//! arm for `can_filter_operators`, so it answers `false` whatever the shell
//! advertises.
//!
//! Phase 1 replaces `capability_flag` with the `Capability` enum; this test
//! becomes a check that every `Capability` reads its own field.

use vantage_vista::mocks::MockShell;
use vantage_vista::{TableShell, VistaCapabilities};

#[test]
#[ignore = "v2 phase 1: the Capability enum replaces the capability_flag string lookup"]
fn capability_flag_complete() {
    let caps = VistaCapabilities {
        can_count: true,
        can_insert: true,
        can_update: true,
        can_delete: true,
        can_import: true,
        can_subscribe: true,
        can_invalidate: true,
        can_order: true,
        can_search: true,
        can_filter_operators: true,
        can_set_page_size: true,
        can_fetch_page: true,
        can_fetch_next: true,
        can_fetch_window: true,
        can_traverse_to_record: true,
        can_traverse_to_set: true,
        can_build_ref_via_script: true,
        can_traverse_in_columns: true,
        can_confine_writes: true,
    };
    let shell = MockShell::new().with_capabilities(caps);
    let missing: Vec<&str> = [
        "can_count",
        "can_insert",
        "can_update",
        "can_delete",
        "can_import",
        "can_subscribe",
        "can_invalidate",
        "can_order",
        "can_search",
        "can_filter_operators",
        "can_set_page_size",
        "can_fetch_page",
        "can_fetch_next",
        "can_fetch_window",
        "can_traverse_to_record",
        "can_traverse_to_set",
        "can_build_ref_via_script",
        "can_traverse_in_columns",
        "can_confine_writes",
    ]
    .into_iter()
    .filter(|name| !shell.capability_flag(name))
    .collect();
    assert!(
        missing.is_empty(),
        "capability_flag answers false for advertised flags: {missing:?}"
    );
}
