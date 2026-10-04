use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;

type Rec = Record<CborValue>;

use crate::mocks::MockShell;
use crate::{Column, TableShell, Vista, VistaMetadata, forward_table_shell};

/// Hides every column and forwards the rest.
struct NoColumns {
    inner: Box<dyn TableShell>,
    columns: IndexMap<String, Column>,
}

forward_table_shell!(NoColumns, inner, {
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        None
    }
    fn columns(&self) -> &IndexMap<String, Column> {
        &self.columns
    }
});

#[tokio::test]
async fn wrapper_overrides_columns_and_forwards_the_rest() {
    let mut row = Record::new();
    row.insert("name".to_string(), CborValue::Text("a".into()));
    let inner = MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("name", "String"))
                .with_id_column("id"),
        )
        .with_record("r1", row);
    let vista = Vista::new(
        "t",
        Box::new(NoColumns {
            inner: Box::new(inner),
            columns: IndexMap::new(),
        }),
    );

    assert!(vista.get_column_names().is_empty());
    assert_eq!(vista.get_id_column(), Some("id"));
    assert!(vista.get_value("r1".to_string()).await.unwrap().is_some());
    assert_eq!(vista.list_values().await.unwrap().len(), 1);
}

/// Lists nothing and refuses every insert and replace.
struct Locked {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Locked, inner, {
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        None
    }
    async fn list_vista_values(&self, _vista: &Vista) -> Result<IndexMap<String, Rec>> {
        Ok(IndexMap::new())
    }
    async fn insert_vista_value(&self, _: &Vista, _: &String, _: &Rec) -> Result<Rec> {
        Err(error!("locked"))
    }
    async fn replace_vista_value(&self, _: &Vista, _: &String, _: &Rec) -> Result<Rec> {
        Err(error!("locked"))
    }
});

#[tokio::test]
async fn stream_and_upsert_go_through_the_wrappers_own_methods() {
    let mut row = Record::new();
    row.insert("name".to_string(), CborValue::Text("a".into()));
    let inner = MockShell::new().with_record("r1", row.clone());
    let shell = Locked {
        inner: Box::new(inner),
    };
    let vista = Vista::new("t", Box::new(MockShell::new()));

    let mut stream = shell.stream_vista_values(&vista);
    let first = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
    assert!(first.is_none(), "the wrapper's empty list is what streams");
    drop(stream);

    let upsert = TableShell::upsert_vista_value(&shell, &vista, &"r2".to_string(), &row).await;
    assert!(upsert.is_err(), "upsert takes the wrapper's refusal");
    assert!(
        shell
            .inner
            .get_vista_value(&vista, &"r2".to_string())
            .await
            .unwrap()
            .is_none()
    );
}
