//! The engine's `table(name)` resolver over the store.

use std::collections::HashMap;

use vantage_memory::TableDef;
use vantage_vista::ColumnFlag;

use super::*;
use crate::FakerColumn;

fn declared() -> HashMap<String, Vec<FakerColumn>> {
    let columns = vec![
        FakerColumn::new("code", "string"),
        FakerColumn::new("n", "int"),
    ];
    HashMap::from([("late".to_string(), columns)])
}

#[test]
fn a_declared_table_created_after_start_gets_its_columns() {
    let store = store_with(&[]);
    let resolver = vocab::memory_resolver(store.clone(), &declared());
    let err = resolver.vista("late").unwrap_err();
    assert!(err.is_not_found(), "{err}");
    assert!(store.table_names().is_empty(), "resolving creates no table");

    store.define(
        "late",
        TableDef {
            id_column: "code".into(),
            ..Default::default()
        },
    );
    let vista = resolver.vista("late").unwrap();
    assert_eq!(vista.id_column(), Some("code"));
    assert!(vista.column("code").unwrap().has_flag(ColumnFlag::Id));
    let n = vista.column("n").unwrap();
    assert!(n.has_flag(ColumnFlag::Orderable) && n.has_flag(ColumnFlag::Searchable));
    assert_eq!(n.original_type, None, "declared columns are untyped");
}

#[test]
fn an_undeclared_store_table_gets_only_its_id_column() {
    let store = store_with(&["log"]);
    let vista = vocab::memory_resolver(store, &declared())
        .vista("log")
        .unwrap();
    assert_eq!(vista.id_column(), Some("id"));
    assert!(vista.columns().is_empty());
}
