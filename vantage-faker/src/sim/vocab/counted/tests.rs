//! `vantage_vista::contract` over a counted memory table, plus the count
//! itself through a clone and a `ref(...)` target.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ciborium::Value as CborValue;
use vantage_dataset::WritableValueSet;
use vantage_memory::vista::Catalog;
use vantage_memory::{MemoryStore, MemoryTableShell};
use vantage_types::Record;
use vantage_vista::{
    Column, ColumnFlag, FilterOp, JoinKey, Reference, ReferenceKind, Vista, VistaMetadata, contract,
};

use super::CountedShell;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn row(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), text(v)))
        .collect()
}

/// Clients c1, c2 and their orders, `client` counted into `writes`.
fn clients(writes: Arc<AtomicU64>) -> Vista {
    let store = MemoryStore::new();
    for (id, kind) in [("c1", "vip"), ("c2", "plain")] {
        store
            .table("client")
            .insert_as(id, row(&[("kind", kind)]))
            .unwrap();
    }
    store
        .table("order")
        .insert_as("o1", row(&[("client", "c1")]))
        .unwrap();
    let catalog = Catalog::new(store.clone());
    catalog.register(
        "order",
        VistaMetadata::new()
            .with_column(Column::new("id", "string").with_flag(ColumnFlag::Id))
            .with_column(Column::new("client", "string"))
            .with_id_column("id"),
    );
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "string").with_flag(ColumnFlag::Id))
        .with_column(Column::new("kind", "string").with_flag(ColumnFlag::Orderable))
        .with_id_column("id")
        .with_reference(Reference::new(
            "orders",
            "order",
            ReferenceKind::HasMany,
            vec![JoinKey::new("id", "client")],
        ));
    let shell = MemoryTableShell::new(store.table("client"), metadata, catalog);
    Vista::new("client", CountedShell::new(shell, writes))
}

#[tokio::test]
async fn counted_contract() {
    let vista = clients(Arc::default());
    contract::check_advertised(&vista).await;
    contract::check_clone(&vista, "kind", text("vip")).await;
    contract::check_advertised_writes(&clients(Arc::default())).await;
}

#[tokio::test]
async fn clones_and_ref_targets_count_their_writes() {
    let writes = Arc::new(AtomicU64::new(0));
    let vista = clients(writes.clone());
    let vips = vista
        .clone()
        .with_condition("kind", FilterOp::Eq, text("vip"))
        .unwrap();
    vips.patch_value("c1", &row(&[("note", "x")]))
        .await
        .unwrap();
    assert_eq!(writes.load(Ordering::Relaxed), 1);

    let c1 = row(&[("id", "c1")]);
    let orders = vista.get_ref("orders", Some(&c1)).unwrap();
    assert!(orders.is_narrowed(), "the join survives the rewrap");
    orders.insert_value("o2", &row(&[])).await.unwrap();
    assert_eq!(writes.load(Ordering::Relaxed), 2);
}
