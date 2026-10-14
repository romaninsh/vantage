//! Facade narrowing: `dio.cached()` honours conditions (any operator), search
//! and order, pushing each into the master where it can answer it and
//! evaluating it over the rows read where it cannot.
//!
//! The CSV cases are the ones that matter most. CSV is the canonical
//! capability-poor master — it advertises no `Order`, and its factory flags
//! no column `Orderable` — so every one of these narrowings is served entirely
//! by the Dio. A mock master with the flags set by hand would pass while the
//! real thing failed.

mod csv;
mod independence;
mod push_down;
#[path = "../support/mod.rs"]
mod support;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_csv::Csv;
use vantage_diorama::Dio;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Capability, Column, ColumnFlag, Vista, VistaMetadata, mocks::MockShell};

// ---- fixtures --------------------------------------------------------------

/// `tests/fixtures/products.csv` — Espresso 4, Cappuccino 5, Latte 6,
/// Flat White 5, in that file order.
fn csv_master() -> Result<Vista> {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let csv = Csv::new(dir);
    let table = Table::<Csv, EmptyEntity>::new("products", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");
    csv.vista_factory().from_table(table)
}

fn row(name: &str, price: i64) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("name".to_string(), CborValue::Text(name.to_string()));
    record.insert("price".to_string(), CborValue::Integer(price.into()));
    record
}

/// A master that *can* order and search server-side, for the push-down side.
fn capable_master() -> MockShell {
    MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("id", "String").with_flag("id"))
                .with_column(Column::new("name", "String").with_flag(ColumnFlag::Orderable))
                .with_column(Column::new("price", "i64").with_flag(ColumnFlag::Orderable))
                .with_id_column("id"),
        )
        .with_capabilities(
            [Capability::Order, Capability::Search]
                .into_iter()
                .collect(),
        )
}

async fn eager_dio(master: Vista) -> Result<Dio> {
    Ok(support::eager_dio(master).await)
}

fn names(rows: &IndexMap<String, Record<CborValue>>) -> Vec<String> {
    rows.values()
        .filter_map(|r| match r.get("name") {
            Some(CborValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .collect()
}
