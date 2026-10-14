//! `limit(n)` must cap the read on a shell that can window rows, with no
//! page-size setting involved.
//!
//! `limit(n)` is a read cap the terminal applies through
//! `fetch_window(0, n)`; there is no page-size capability. A shell that
//! answers only the leading window is never listed in full for it.
//!
//! A refused leading window is an error, never a truncated list.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_rhai::Host;
use vantage_rhai::rhai::{Array, Map as RhaiMap};
use vantage_types::Record;
use vantage_vista::{
    Capability, Page, TableShell, VistaCapabilities, forward_table_shell, unsupported,
};

use super::rhai_support::{host, mock, reads, run};

fn listed_ids(host: &Host, script: &str) -> Vec<String> {
    let rows = run(host, script).unwrap_or_else(|e| panic!("`{script}` failed: {e}"));
    let rows = rows.try_cast::<Array>().expect("list() returns an array");
    rows.into_iter()
        .map(|row| {
            let row = row.try_cast::<RhaiMap>().expect("each row is a map");
            row["id"].clone().into_string().expect("id is a string")
        })
        .collect()
}

/// Lists, but refuses every window.
#[derive(Clone)]
struct NoWindow {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(NoWindow, inner, {
    async fn fetch_window(&self, _offset: usize, _limit: usize) -> Result<Page> {
        unsupported(Capability::Window)
    }
});

#[test]
fn a_refused_leading_window_is_an_error_never_a_truncated_list() {
    let host = host(
        NoWindow {
            inner: Box::new(mock()),
        },
        reads(),
    );
    let err = run(&host, r#"table("t").limit(2).list()"#).unwrap_err();
    assert!(!err.is_empty());
    assert!(
        run(&host, r#"table("t").list()"#).is_ok(),
        "an uncapped list still works"
    );
}

/// Counts the full listings the shell is asked for.
#[derive(Clone)]
struct ListSpy {
    inner: Box<dyn TableShell>,
    lists: Arc<AtomicUsize>,
}

forward_table_shell!(ListSpy, inner, {
    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        self.lists.fetch_add(1, Ordering::SeqCst);
        self.inner.list().await
    }
});

#[test]
fn rhai_limit_reads_the_leading_window() {
    let lists = Arc::new(AtomicUsize::new(0));
    let spy = ListSpy {
        inner: Box::new(mock().with_capabilities(VistaCapabilities::new())),
        lists: lists.clone(),
    };
    let host = host(spy, reads());
    for script in [
        r#"table("t").limit(2).list()"#,
        r#"table("t").limit(2).ids()"#,
        r#"[table("t").first()]"#,
    ] {
        let rows = run(&host, script).unwrap_or_else(|e| panic!("`{script}` failed: {e}"));
        let rows = rows.try_cast::<Array>().expect("an array");
        assert!(rows.len() <= 2, "`{script}` read {} rows", rows.len());
    }
    assert_eq!(
        listed_ids(&host, r#"table("t").limit(2).list()"#),
        ["r1", "r2"]
    );
    assert_eq!(
        lists.load(Ordering::SeqCst),
        0,
        "a capped read must not list the whole table"
    );
}
