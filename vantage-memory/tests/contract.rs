//! `vantage_vista::contract` over the in-memory store.

use ciborium::Value as CborValue;
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_types::Record;
use vantage_vista::{VistaFactory, contract};

const PRODUCT: &str = r#"
name: product
id_column: id
columns:
  id: { type: string, flags: [id, orderable] }
  name: { type: string, flags: [title, searchable] }
  category: { type: string }
"#;

const ORDER: &str = r#"
name: order
id_column: id
columns:
  id: { type: string, flags: [id] }
  product: { type: string }
references:
  product: { table: product, kind: has_one, foreign_key: product }
"#;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn record(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), text(v)))
        .collect()
}

/// Three products, two of them drinks, and one order pointing at a product.
fn factory() -> MemoryVistaFactory {
    let store = MemoryStore::new();
    let products = store.table("product");
    for (id, name, category) in [
        ("p1", "Tea", "drink"),
        ("p2", "Cake", "food"),
        ("p3", "Coffee", "drink"),
    ] {
        products.upsert(id, record(&[("name", name), ("category", category)]));
    }
    store
        .table("order")
        .upsert("o1", record(&[("product", "p1")]));
    MemoryVistaFactory::new(store)
}

#[tokio::test]
async fn memory_contract_advertised() {
    let f = factory();
    contract::check_advertised(&f.from_yaml(PRODUCT).unwrap()).await;
    contract::check_advertised(&f.from_yaml(ORDER).unwrap()).await;
    contract::check_advertised_writes(&f.from_yaml(PRODUCT).unwrap()).await;
}

#[tokio::test]
async fn memory_contract_clone() {
    let vista = factory().from_yaml(PRODUCT).unwrap();
    contract::check_clone(&vista, "category", text("drink")).await;
}
