use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_types::Record;

type Rec = Record<CborValue>;

use crate::mocks::MockShell;
use crate::{Column, FilterOp, TableShell, Vista, VistaMetadata, forward_table_shell};

/// Hides every column and forwards the rest.
#[derive(Clone)]
struct NoColumns {
    inner: Box<dyn TableShell>,
    columns: IndexMap<String, Column>,
}

forward_table_shell!(NoColumns, inner, {
    fn columns(&self) -> &IndexMap<String, Column> {
        &self.columns
    }
});

fn row(name: &str) -> Rec {
    [("name".to_string(), CborValue::Text(name.into()))]
        .into_iter()
        .collect()
}

#[tokio::test]
async fn wrapper_overrides_columns_and_forwards_the_rest() {
    let inner = MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("name", "String"))
                .with_id_column("id"),
        )
        .with_record("r1", row("a"));
    let vista = Vista::new("t", inner).map_shell(|inner| NoColumns {
        inner,
        columns: IndexMap::new(),
    });

    assert!(vista.columns().is_empty());
    assert_eq!(vista.id_column(), Some("id"));
    assert!(vista.get_value("r1").await.unwrap().is_some());
    assert_eq!(vista.list_values().await.unwrap().len(), 1);
}

/// A wrapped Vista keeps its name and applied query, and a clone of the
/// wrapper carries the inner shell's narrowing.
#[tokio::test]
async fn map_shell_keeps_the_query_and_clones_narrow_independently() {
    let inner = MockShell::new()
        .with_record("r1", row("a"))
        .with_record("r2", row("b"));
    let narrowed = Vista::new("t", inner)
        .with_condition("name", FilterOp::Eq, "a")
        .unwrap()
        .map_shell(|inner| NoColumns {
            inner,
            columns: IndexMap::new(),
        });
    assert_eq!(narrowed.name(), "t");
    assert!(narrowed.is_narrowed());
    assert_eq!(narrowed.query().conditions.len(), 1);

    let copy = narrowed
        .clone()
        .with_condition("name", FilterOp::Eq, "b")
        .unwrap();
    assert!(copy.list_values().await.unwrap().is_empty());
    assert_eq!(narrowed.list_values().await.unwrap().len(), 1);
}

/// Lists nothing and refuses every insert.
#[derive(Clone)]
struct Locked {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Locked, inner, {
    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        Ok(IndexMap::new())
    }
    async fn insert(&self, _id: Option<&str>, _record: &Rec) -> Result<(String, Rec)> {
        Err(error!("locked"))
    }
});

#[tokio::test]
async fn overridden_methods_win_over_the_inner_shell() {
    let shell = Locked {
        inner: Box::new(MockShell::new().with_record("r1", row("a"))),
    };
    let vista = Vista::new("t", shell);
    assert!(vista.list_values().await.unwrap().is_empty());
    assert!(vista.insert_value("r2", &row("b")).await.is_err());
    assert!(vista.get_value("r2").await.unwrap().is_none());
    assert!(
        vista.get_value("r1").await.unwrap().is_some(),
        "get forwards"
    );
}
