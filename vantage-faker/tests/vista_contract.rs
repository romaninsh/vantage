//! `vantage_vista::contract` over shaped memory tables: one shape that
//! advertises everything it can, one that pages by cursor only.

use ciborium::Value as CborValue;
use vantage_faker::{BackendShape, ShapedShell};
use vantage_memory::vista::Catalog;
use vantage_memory::{MemoryStore, MemoryTableShell};
use vantage_types::Record;
use vantage_vista::{Capability, Column, ColumnFlag, Vista, VistaMetadata, contract};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

/// Three items, two of them drinks, shaped by `shape`.
fn items(shape: BackendShape) -> Vista {
    let store = MemoryStore::new();
    let table = store.table("item");
    for (id, name, category) in [
        ("i1", "Tea", "drink"),
        ("i2", "Cake", "food"),
        ("i3", "Coffee", "drink"),
    ] {
        let row: Record<CborValue> = [
            ("name".to_string(), text(name)),
            ("category".to_string(), text(category)),
        ]
        .into_iter()
        .collect();
        table.insert_as(id, row).unwrap();
    }
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "string").with_flag(ColumnFlag::Id))
        .with_column(
            Column::new("name", "string")
                .with_flag(ColumnFlag::Title)
                .with_flag(ColumnFlag::Searchable)
                .with_flag(ColumnFlag::Orderable),
        )
        .with_column(Column::new("category", "string").with_flag(ColumnFlag::Orderable))
        .with_id_column("id");
    let shell = MemoryTableShell::new(table, metadata, Catalog::new(store));
    Vista::new("item", shell).map_shell(|inner| ShapedShell::new(inner, shape))
}

fn everything() -> BackendShape {
    BackendShape {
        capabilities: Capability::iter().collect(),
        seed: Some(1),
        ..BackendShape::default()
    }
}

fn cursor_only() -> BackendShape {
    BackendShape {
        capabilities: [Capability::Cursor].into_iter().collect(),
        page_size: 2,
        seed: Some(1),
        ..BackendShape::default()
    }
}

#[tokio::test]
async fn faker_contract_advertised() {
    contract::check_advertised(&items(everything())).await;
    contract::check_advertised(&items(cursor_only())).await;
    contract::check_advertised(&items(BackendShape::default())).await;
}

#[tokio::test]
async fn faker_contract_advertised_writes() {
    contract::check_advertised_writes(&items(everything())).await;
    contract::check_advertised_writes(&items(BackendShape::default())).await;
}

#[tokio::test]
async fn faker_contract_clone() {
    contract::check_clone(&items(everything()), "category", text("drink")).await;
    contract::check_clone(&items(cursor_only()), "category", text("drink")).await;
}
