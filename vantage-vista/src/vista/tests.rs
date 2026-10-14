//! What a `Vista` records of the clauses applied to it.

use ciborium::Value as CborValue;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;

use crate::mocks::MockShell;
use crate::{
    Capability, Column, ColumnFlag, Condition, FilterOp, NativeCondition, SortDirection,
    TableShell, Vista, VistaCapabilities, VistaMetadata, VistaQuery, forward_table_shell,
};

mod reads;

fn row(n: i64, name: &str) -> Record<CborValue> {
    [
        ("n".to_string(), CborValue::Integer(n.into())),
        ("name".to_string(), CborValue::Text(name.into())),
    ]
    .into_iter()
    .collect()
}

/// Rows r1..r3 with `n` 1..3; `n` is orderable.
fn shell() -> MockShell {
    MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("n", "int").with_flag(ColumnFlag::Orderable))
                .with_column(Column::new("name", "String")),
        )
        .with_record("r1", row(1, "a"))
        .with_record("r2", row(2, "b"))
        .with_record("r3", row(3, "c"))
}

/// Takes equality only: no FilterOps, Search or Order.
fn refusing() -> Vista {
    let caps = VistaCapabilities::new().with(Capability::Window);
    Vista::new("t", shell().with_capabilities(caps))
}

#[test]
fn a_refused_clause_is_not_recorded() {
    let mut vista = refusing();
    let err = vista.add_condition("n", FilterOp::Gt, 1).unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    assert!(vista.set_search(Some("a")).unwrap_err().is_unsupported());
    assert!(
        vista
            .set_order(Some(("n", SortDirection::Ascending)))
            .unwrap_err()
            .is_unsupported()
    );
    assert!(vista.query().is_empty());
    assert!(!vista.is_narrowed());
}

/// Clearing what was never applied is a no-op, even on a shell without the
/// capability.
#[test]
fn clearing_nothing_is_a_no_op() {
    let mut vista = refusing();
    vista.set_search(None).unwrap();
    vista.set_order(None).unwrap();
    assert!(vista.query().is_empty());
}

#[tokio::test]
async fn set_search_and_set_order_replace_and_clear() {
    let mut vista = Vista::new("t", shell());
    vista.set_search(Some("a")).unwrap();
    vista.set_search(Some("b")).unwrap();
    assert_eq!(vista.query().search.as_deref(), Some("b"));
    assert_eq!(vista.list_values().await.unwrap().len(), 1);
    vista.set_search(None).unwrap();
    assert_eq!(vista.query().search, None);
    assert_eq!(vista.list_values().await.unwrap().len(), 3);

    vista
        .set_order(Some(("n", SortDirection::Descending)))
        .unwrap();
    let first = vista.list_values().await.unwrap().into_keys().next();
    assert_eq!(first.as_deref(), Some("r3"));
    vista.set_order(None).unwrap();
    assert_eq!(vista.query().order, None);
}

#[tokio::test]
async fn with_query_returns_what_the_shell_refused() {
    let query = VistaQuery::new()
        .with_condition(Condition::new("name", FilterOp::Eq, "b"))
        .with_condition(Condition::new("n", FilterOp::Gt, 1))
        .with_search("b")
        .with_order("n", SortDirection::Descending);
    let (vista, residual) = refusing().with_query(&query).unwrap();

    assert_eq!(vista.query().conditions, query.conditions[..1]);
    assert_eq!(residual.conditions, query.conditions[1..]);
    assert_eq!(residual.search.as_deref(), Some("b"));
    assert_eq!(residual.order, query.order);
    assert_eq!(vista.list_values().await.unwrap().len(), 1);
}

#[test]
fn with_query_returns_other_errors() {
    let unknown_column = VistaQuery::new().with_order("missing", SortDirection::Ascending);
    let err = refusing().with_query(&unknown_column).unwrap_err();
    assert!(!err.is_unsupported(), "{err}");
}

/// Fails every condition with a plain error, neither a refusal nor
/// unimplemented.
#[derive(Clone)]
struct Failing {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Failing, inner, {
    fn add_condition(&mut self, _condition: &Condition) -> vantage_core::Result<()> {
        Err(vantage_core::error!("backend rejected the condition"))
    }
});

#[test]
fn with_query_propagates_a_shell_error() {
    let query = VistaQuery::new().with_condition(Condition::new("name", FilterOp::Eq, "b"));
    let err = Vista::new("t", shell())
        .map_shell(|inner| Failing { inner })
        .with_query(&query)
        .unwrap_err();
    assert!(!err.is_unsupported() && !err.is_unimplemented(), "{err:?}");
    assert!(
        err.to_string().contains("backend rejected the condition"),
        "{err}"
    );
}

/// Validity before capability: an undeclared column is a plain error even
/// where the capability is missing too.
#[test]
fn an_undeclared_column_is_an_error_not_a_refusal() {
    let mut vista = refusing();
    let condition = vista.add_condition("missing", FilterOp::Gt, 1).unwrap_err();
    assert!(!condition.is_unsupported(), "{condition}");
    let order = vista
        .set_order(Some(("missing", SortDirection::Ascending)))
        .unwrap_err();
    assert!(!order.is_unsupported(), "{order}");

    // A path into a declared column is valid (refused for the operator); one
    // into an undeclared root is an error.
    let declared = vista
        .add_condition("name.first", FilterOp::Gt, "a")
        .unwrap_err();
    assert!(declared.is_unsupported(), "{declared}");
    let nested = vista
        .add_condition("missing.first", FilterOp::Gt, "a")
        .unwrap_err();
    assert!(!nested.is_unsupported(), "{nested}");
}

/// Where no refusal is pending, the shell owns its column vocabulary: an
/// equality on a column the Vista doesn't declare reaches the shell, which
/// decides. A schemaless mock takes it and the Vista records it; a mock
/// with a schema rejects it.
#[test]
fn an_undeclared_equality_column_is_the_shells_call() {
    let caps = VistaCapabilities::new().with(Capability::Window);
    let vista = Vista::new("t", MockShell::new().with_capabilities(caps))
        .with_condition("placeholder", FilterOp::Eq, "x")
        .unwrap();
    assert_eq!(
        vista.query().conditions,
        [Condition::new("placeholder", FilterOp::Eq, "x")]
    );
    let err = refusing()
        .with_condition("placeholder", FilterOp::Eq, "x")
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err}");
}

#[test]
fn an_unflagged_sort_column_is_a_refusal() {
    let mut vista = Vista::new("t", shell());
    let err = vista
        .set_order(Some(("name", SortDirection::Ascending)))
        .unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    assert!(vista.query().is_empty());
}

#[test]
fn queries_with_native_conditions_cannot_be_re_applied() {
    let query = VistaQuery {
        native: vec!["k".to_string()],
        ..VistaQuery::default()
    };
    let err = Vista::new("t", shell()).with_query(&query).unwrap_err();
    assert!(err.is_incorrect_usage(), "{err:?}");
    assert!(err.to_string().contains("native"), "{err}");
}

/// Takes any native condition and records nothing else.
#[derive(Clone)]
struct Native {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Native, inner, {
    fn add_native_condition(&mut self, _condition: NativeCondition) -> vantage_core::Result<()> {
        Ok(())
    }
});

#[test]
fn native_conditions_are_recorded_by_key() {
    let vista = Vista::new("t", shell())
        .map_shell(|inner| Native { inner })
        .with_native_condition(NativeCondition::new((), "is_paying = true"))
        .unwrap();
    assert_eq!(vista.query().native, ["is_paying = true"]);
    assert!(vista.is_narrowed());
    let other = Vista::new("t", shell())
        .map_shell(|inner| Native { inner })
        .with_native_condition(NativeCondition::new((), "is_paying = false"))
        .unwrap();
    assert_ne!(vista.query().key(), other.query().key());
}

#[derive(Clone)]
struct Wrapped {
    inner: Box<dyn TableShell>,
    metadata: VistaMetadata,
}

forward_table_shell!(Wrapped, inner, {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }
});

#[test]
fn map_shell_keeps_the_name_and_applied_query() {
    let vista = Vista::new("t", shell())
        .with_condition("name", FilterOp::Eq, "a")
        .unwrap()
        .with_order("n", SortDirection::Ascending)
        .unwrap();
    let applied = vista.query().clone();
    let wrapped = vista.map_shell(|inner| Wrapped {
        inner,
        metadata: VistaMetadata::new(),
    });
    assert_eq!(wrapped.name(), "t");
    assert_eq!(wrapped.query(), &applied);
}

/// The mock shell would run every write; the Vista refuses each one its
/// shell doesn't advertise, before the shell sees it.
#[tokio::test]
async fn writes_the_shell_does_not_advertise_are_refused() {
    use vantage_dataset::traits::{InsertableValueSet, WritableValueSet};

    let store = shell();
    let caps = VistaCapabilities::new().with(Capability::Window);
    let vista = Vista::new("t", store.clone().with_capabilities(caps));
    let rec = row(9, "z");
    let refusals = [
        ("insert", vista.insert_value("r9", &rec).await.map(drop)),
        (
            "insert(None)",
            vista.insert_return_id_value(&rec).await.map(drop),
        ),
        ("replace", vista.replace_value("r1", &rec).await.map(drop)),
        ("upsert", vista.upsert_value("r1", &rec).await.map(drop)),
        ("patch", vista.patch_value("r1", &rec).await.map(drop)),
        ("delete", vista.delete("r1").await),
        ("delete_all", vista.delete_all().await),
        (
            "import",
            vista
                .import_values(&[("r9".to_string(), rec.clone())].into_iter().collect())
                .await
                .map(drop),
        ),
    ];
    for (verb, result) in refusals {
        let err = result.expect_err(verb);
        assert!(err.is_unsupported(), "{verb}: {err:?}");
    }
    assert_eq!(store.len(), 3, "no write reached the shell");
    assert_eq!(vista.list_values().await.unwrap().len(), 3);
}

#[tokio::test]
async fn an_advertised_write_still_runs() {
    use vantage_dataset::traits::WritableValueSet;

    let caps = VistaCapabilities::new().with(Capability::Delete);
    let vista = Vista::new("t", shell().with_capabilities(caps));
    vista.delete("r1").await.unwrap();
    assert_eq!(vista.list_values().await.unwrap().len(), 2);
}
