//! Shared test scaffolding.

use std::sync::Arc;

use vantage_dataset::traits::WritableValueSet;
use vantage_redb::{AnyRedbType, Redb};
use vantage_table::any::AnyTable;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

/// Build a tempfile-backed redb master with three rows already inserted.
/// Returns the tempfile (keep it alive for the test's duration), the
/// AnyTable wrapper, and the typed Table for direct writes outside the
/// LiveTable wrapper.
pub async fn seeded_redb_master(
    table_name: &str,
) -> (tempfile::NamedTempFile, AnyTable, Table<Redb, EmptyEntity>) {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();

    let typed = Table::<Redb, EmptyEntity>::new(table_name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");

    for (id, name, price) in [
        ("a", "Alpha", 10i64),
        ("b", "Beta", 20i64),
        ("c", "Gamma", 30i64),
    ] {
        let mut r: Record<AnyRedbType> = Record::new();
        r.insert("name".into(), AnyRedbType::new(name.to_string()));
        r.insert("price".into(), AnyRedbType::new(price));
        typed.insert_value(&id.to_string(), &r).await.unwrap();
    }

    let any = AnyTable::from_table(typed.clone());
    (path, any, typed)
}

/// Initialise tracing so `RUST_LOG=vantage_live=debug cargo test --
/// --nocapture` prints span output. Idempotent — safe to call from
/// multiple tests in the same process.
#[allow(dead_code)]
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("vantage_live=info")),
        )
        .with_test_writer()
        .try_init();
}

/// Convenience: wrap a CBOR string into a single-field record. Useful
/// for asserting on returned rows where the field name and type are
/// known.
#[allow(dead_code)]
pub fn cbor_string(value: &str) -> ciborium::Value {
    ciborium::Value::Text(value.to_string())
}

#[allow(dead_code)]
pub fn arc_cache<C: vantage_live::Cache + 'static>(cache: C) -> Arc<dyn vantage_live::Cache> {
    Arc::new(cache)
}
