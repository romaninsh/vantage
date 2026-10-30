use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_types::Record;
use vantage_vista::{
    JoinKey, Reference, TableShell, Transaction, Vista, VistaMetadata, forward_table_shell,
    mocks::MockShell,
};

/// A shell that rebinds to itself: what a datasource without transactions does.
#[derive(Clone)]
struct Rebindable {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Rebindable, inner, {
    fn rebind(&self, _tx: &Transaction) -> Result<Box<dyn TableShell>> {
        Ok(Box::new(self.clone()))
    }
});

fn rebindable(name: &str, shell: MockShell) -> Vista {
    Vista::new(name, shell).map_shell(|inner| Rebindable { inner })
}

fn rec(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).into(), CborValue::Text((*v).into())))
        .collect()
}

#[tokio::test]
async fn a_shell_without_rebind_is_refused_at_first_use() {
    let tx = Transaction::new();
    let orders = tx.wrap(&Vista::new("orders", MockShell::new()));

    let err = orders.list_values().await.unwrap_err();
    assert!(
        err.to_string()
            .contains("\"orders\" can't take part in a transaction"),
        "{err}"
    );
}

#[tokio::test]
async fn wrapped_get_ref_target_is_wrapped() {
    let metadata = VistaMetadata::new().with_reference(Reference::has_many(
        "lines",
        "lines",
        JoinKey::new("id", "order_id"),
    ));
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_ref_target("lines", MockShell::new());
    let tx = Transaction::new();
    let orders = tx.wrap(&rebindable("orders", shell));

    let lines = orders.get_ref("lines", None).unwrap();
    let err = lines.list_values().await.unwrap_err();
    assert!(
        err.to_string().contains("can't take part in a transaction"),
        "{err}"
    );
}

#[tokio::test]
async fn use_after_finish_errors() {
    let tx = Transaction::new();
    let orders = tx.wrap(&rebindable("orders", MockShell::new()));
    tx.commit().await.unwrap();

    let err = orders.list_values().await.unwrap_err();
    assert!(err.to_string().contains("transaction finished"), "{err}");
}

#[tokio::test]
async fn writes_are_reported() {
    let tx = Transaction::new();
    let orders = tx.wrap(&rebindable("orders", MockShell::new()));
    orders
        .insert_value("1", &rec(&[("item", "pen")]))
        .await
        .unwrap();

    let committed = tx.commit().await.unwrap();
    assert_eq!(committed.written, vec!["orders".to_string()]);
}

#[test]
fn lock_on_a_shell_without_lock_is_refused() {
    let mut orders = Vista::new("orders", MockShell::new());
    let err = orders.lock().unwrap_err();
    assert!(err.is_unsupported(), "{err}");
}
