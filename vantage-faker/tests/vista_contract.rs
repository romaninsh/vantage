//! `vantage_vista::contract` over shaped memory tables: one shape that
//! advertises everything it can, one that pages by cursor only.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use vantage_faker::{BackendShape, ShapedShell};
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_types::Record;
use vantage_vista::contract::{TxFixture, TxTables};
use vantage_vista::{Capability, Vista, VistaFactory, contract, direct};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

/// Three items, two of them drinks, shaped by `shape`. `origin` is a
/// declared map column that i1 and i3 fill and i2 lacks, so the contract's
/// dotted-path probe has a row to find and a row to leave out.
fn items(shape: BackendShape) -> Vista {
    let store = MemoryStore::new();
    let table = store.table("item");
    for (id, name, category, origin) in [
        ("i1", "Tea", "drink", Some("India")),
        ("i2", "Cake", "food", None),
        ("i3", "Coffee", "drink", Some("Kenya")),
    ] {
        let mut row: Record<CborValue> = [
            ("name".to_string(), text(name)),
            ("category".to_string(), text(category)),
        ]
        .into_iter()
        .collect();
        if let Some(country) = origin {
            let map = CborValue::Map(vec![(text("country"), text(country))]);
            row.insert("origin".to_string(), map);
        }
        table.insert_as(id, row).unwrap();
    }
    MemoryVistaFactory::new(store)
        .from_yaml(ITEM)
        .unwrap()
        .map_shell(|inner| ShapedShell::new(inner, shape))
}

const ITEM: &str = r#"
name: item
id_column: id
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title, searchable] }
  category: { type: string }
  origin: {}
"#;

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

/// A shaped table over a datasource without transactions, stood in for by
/// a memory table made [`direct`]: the shape hands its rebind to the inner
/// shell, which takes part directly.
fn shaped_direct(store: &MemoryStore, name: &str) -> Vista {
    let yaml = format!(
        "name: {name}\ncolumns:\n  id: {{ type: string, flags: [id] }}\n  n: {{ type: integer }}\n"
    );
    MemoryVistaFactory::new(store.clone())
        .from_yaml(&yaml)
        .unwrap()
        .map_shell(|inner| ShapedShell::new(direct(inner), everything()))
}

struct Shaped;

#[async_trait]
impl TxFixture for Shaped {
    async fn fresh(&self) -> TxTables {
        let store = MemoryStore::new();
        TxTables::new(shaped_direct(&store, "a"), shaped_direct(&store, "b"))
    }

    async fn fresh_pool_of_one(&self) -> TxTables {
        self.fresh().await
    }

    fn detects_deadlocks(&self) -> bool {
        false
    }

    fn supports_transactions(&self) -> bool {
        false
    }
}

/// Without transactions both contracts run the same direct writes, so one
/// test checks both.
#[tokio::test]
async fn faker_transactions_and_atomic_hold_the_contract() {
    contract::check_transactions(&Shaped).await;
    #[cfg(feature = "sim")]
    contract::check_atomic_rhai(&Shaped).await;
}
