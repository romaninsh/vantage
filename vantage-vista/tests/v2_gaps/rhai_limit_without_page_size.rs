//! `limit(n)` must cap the read on a shell that can window rows, with no
//! page-size setting involved.
//!
//! In 1.x the `limit` step called `Vista::set_page_size` while the handle
//! resolved, so a shell without `can_set_page_size` failed before the
//! terminal ran. In 2.0 `limit(n)` is a read cap the terminal applies through
//! `fetch_window(0, n)`, and there is no page-size capability.

use vantage_rhai::rhai::{Array, Map as RhaiMap};
use vantage_vista::{Capability, Terminals, VistaCapabilities};

use super::rhai_support::{host, mock, run};

#[test]
fn rhai_limit_without_page_size() {
    let caps = VistaCapabilities::new().with(Capability::Window);
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
