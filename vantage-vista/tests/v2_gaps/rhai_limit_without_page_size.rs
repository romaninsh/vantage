//! `limit(n)` on a shell that can window rows but can't set a page size must
//! still cap the read.
//!
//! Today the `limit` step calls `Vista::set_page_size` while the handle
//! resolves, so a shell without `can_set_page_size` fails before the terminal
//! runs. In 2.0 `limit(n)` is a read cap the terminal applies through
//! `fetch_window(0, n)`.
//!
//! Phase 1 removes `can_count`, `can_fetch_window` and `can_set_page_size`:
//! the caps become `{Window}` with no page-size capability; the assertion
//! stays.

use vantage_rhai::rhai::{Array, Map as RhaiMap};
use vantage_vista::{Terminals, VistaCapabilities};

use super::rhai_support::{host, mock, run};

#[test]
#[ignore = "v2 phase 2: limit(n) becomes a read cap instead of set_page_size"]
fn rhai_limit_without_page_size() {
    let caps = VistaCapabilities {
        can_count: true,
        can_fetch_window: true,
        can_set_page_size: false,
        ..VistaCapabilities::default()
    };
    let host = host(
        mock().with_capabilities(caps),
        Terminals::Read { limit: None },
    );
    let rows = run(&host, r#"table("t").limit(2).list()"#)
        .unwrap_or_else(|e| panic!("limit(2).list() failed: {e}"));
    let rows = rows.try_cast::<Array>().expect("list() returns an array");
    let ids: Vec<String> = rows
        .into_iter()
        .map(|row| {
            let row = row.try_cast::<RhaiMap>().expect("each row is a map");
            row["id"].clone().into_string().expect("id is a string")
        })
        .collect();
    assert_eq!(ids, ["r1", "r2"], "limit(2) must return the first 2 rows");
}
