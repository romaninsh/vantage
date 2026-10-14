//! A capped read is a leading window, never a truncated full list.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;

use crate::mocks::MockShell;
use crate::rhai::read::fetch_rows;
use crate::{
    Capability, Page, TableShell, Vista, VistaCapabilities, forward_table_shell, unsupported,
};

fn rows() -> MockShell {
    let row = |n: i64| -> Record<CborValue> {
        [("n".to_string(), CborValue::Integer(n.into()))]
            .into_iter()
            .collect()
    };
    MockShell::new()
        .with_record("a", row(1))
        .with_record("b", row(2))
        .with_record("c", row(3))
}

/// Answers a leading window but fails every full list.
#[derive(Clone)]
struct NoList {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(NoList, inner, {
    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        Err(error!("list must not run for a capped read"))
    }
});

/// Refuses every window.
#[derive(Clone)]
struct NoWindow {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(NoWindow, inner, {
    async fn fetch_window(&self, _offset: usize, _limit: usize) -> Result<Page> {
        unsupported(Capability::Window)
    }
});

fn vista(caps: VistaCapabilities) -> Vista {
    Vista::new("t", rows().with_capabilities(caps))
}

#[tokio::test]
async fn a_leading_window_serves_a_shell_without_window() {
    let vista = vista(VistaCapabilities::new()).map_shell(|inner| NoList { inner });
    let rows = fetch_rows(&vista, Some(2)).await.unwrap();
    let ids: Vec<String> = rows.into_iter().map(|(id, _)| id).collect();
    assert_eq!(ids, ["a", "b"]);
}

#[tokio::test]
async fn a_refused_leading_window_is_an_error() {
    for caps in [
        VistaCapabilities::new(),
        VistaCapabilities::new().with(Capability::Window),
    ] {
        let vista = vista(caps).map_shell(|inner| NoWindow { inner });
        let err = fetch_rows(&vista, Some(2)).await.unwrap_err();
        assert!(err.is_unsupported(), "{err}");
    }
}
