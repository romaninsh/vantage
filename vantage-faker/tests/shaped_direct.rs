//! `ShapedShell` called directly, as a forward wrapper or diorama calls a
//! shell: the simulated backend's refusals hold without `Vista` in front,
//! and the leading window is answered whatever the shape lists.

use ciborium::Value as CborValue;
use vantage_faker::{BackendShape, ShapedShell};
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_types::Record;
use vantage_vista::{Capability, SortDirection, TableShell, VistaFactory};

/// Three rows behind a shape that lists no capability at all.
fn bare_shape() -> ShapedShell {
    let store = MemoryStore::new();
    let table = store.table("t");
    for id in ["a", "b", "c"] {
        table.insert_as(id, Record::new()).unwrap();
    }
    let shape = BackendShape {
        capabilities: Default::default(),
        seed: Some(1),
        ..BackendShape::default()
    };
    // `map_shell` hands over the bare memory shell; keep the shape built on it.
    let mut shaped = None;
    MemoryVistaFactory::new(store)
        .from_yaml("name: t\ncolumns:\n  id: { type: string, flags: [id] }\n")
        .unwrap()
        .map_shell(|inner| shaped.insert(ShapedShell::new(inner, shape)).clone());
    shaped.unwrap()
}

fn refused<T>(result: vantage_core::Result<T>, capability: Capability) {
    match result {
        Ok(_) => panic!("{capability} must be refused"),
        Err(e) => assert!(e.is_unsupported(), "{capability}: {e}"),
    }
}

#[tokio::test]
async fn the_leading_window_needs_no_window_capability() {
    let shell = bare_shape();
    assert!(!shell.capabilities().has(Capability::Window));
    let page = shell.fetch_window(0, 2).await.unwrap();
    assert_eq!(page.rows.len(), 2);
    refused(shell.fetch_window(1, 2).await, Capability::Window);
}

#[tokio::test]
async fn direct_calls_keep_the_shape_refusals() {
    let mut shell = bare_shape();
    let empty = Record::<CborValue>::new();
    refused(shell.insert(None, &empty).await, Capability::Insert);
    refused(shell.patch("a", &empty).await, Capability::Update);
    refused(shell.delete("a").await, Capability::Delete);
    refused(shell.set_search(Some("x")), Capability::Search);
    refused(
        shell.set_order(Some(("id", SortDirection::Ascending))),
        Capability::Order,
    );
    refused(shell.fetch_cursor(None, 2).await, Capability::Cursor);
    refused(shell.watch().await, Capability::Subscribe);
    assert_eq!(shell.list().await.unwrap().len(), 3, "nothing was written");
}
