//! `AggregationCatalog`: building an aggregation from its configured name.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::aggregate::{
    Aggregation, AggregationCatalog, AggregationSpec, Conditions, Count, Reads, Rows,
    ScalarAggregation, Sum, Where,
};
use vantage_vista::VistaMetadata;

use crate::support::aggregate::{as_i64, int, master, record, settle, source, text};

/// Four orders: three open, one done.
fn orders() -> Rows {
    [
        ("a", "open", 10),
        ("b", "done", 5),
        ("c", "open", 7),
        ("d", "open", 7),
    ]
    .into_iter()
    .map(|(id, status, amount)| {
        let row = record(&[("status", text(status)), ("amount", int(amount))]);
        (id.to_string(), row)
    })
    .collect()
}

fn on(column: &str) -> AggregationSpec {
    AggregationSpec::new(Some(column.to_string()), Conditions::new())
}

fn bare() -> AggregationSpec {
    AggregationSpec::new(None, Conditions::new())
}

fn open() -> Conditions {
    Conditions::new().eq("status", "open")
}

fn computed(catalog: &AggregationCatalog, name: &str, spec: AggregationSpec) -> CborValue {
    catalog
        .build(name, spec)
        .expect("builds")
        .compute(&orders(), &VistaMetadata::new())
}

#[test]
fn the_builtins_are_listed_by_their_lowercase_names() {
    let catalog = AggregationCatalog::with_builtins();
    assert_eq!(
        catalog.names(),
        ["avg", "count", "distinct", "max", "min", "sum"]
    );
}

#[test]
fn a_default_catalog_holds_the_builtins_and_a_new_one_holds_nothing() {
    assert_eq!(
        AggregationCatalog::default().names(),
        AggregationCatalog::with_builtins().names()
    );
    assert!(AggregationCatalog::new().names().is_empty());
}

#[test]
fn each_builtin_reduces_like_its_typed_constructor() {
    let catalog = AggregationCatalog::with_builtins();
    assert_eq!(computed(&catalog, "count", bare()), int(4));
    assert_eq!(computed(&catalog, "sum", on("amount")), int(29));
    assert_eq!(
        computed(&catalog, "avg", on("amount")),
        CborValue::Float(7.25)
    );
    assert_eq!(computed(&catalog, "min", on("amount")), int(5));
    assert_eq!(computed(&catalog, "max", on("amount")), int(10));
    assert_eq!(computed(&catalog, "distinct", on("amount")), int(3));
}

#[test]
fn a_reduction_without_its_column_is_an_error_naming_the_op() {
    let catalog = AggregationCatalog::with_builtins();
    for op in ["sum", "avg", "min", "max", "distinct"] {
        let err = catalog.build(op, bare()).err().expect("needs a column");
        let message = err.to_string();
        assert!(message.contains("column"), "{op}: {message}");
        assert!(message.contains(op), "names the op: {message}");
    }
}

#[test]
fn an_unknown_name_is_an_error_listing_what_is_available() {
    let catalog = AggregationCatalog::with_builtins();
    let err = catalog
        .build("median", on("amount"))
        .err()
        .expect("unknown");
    let message = err.to_string();
    assert!(message.contains("median"), "{message}");
    assert!(
        message.contains("avg, count, distinct, max, min, sum"),
        "{message}"
    );
}

#[test]
fn the_spec_conditions_narrow_any_named_reduction() {
    let catalog = AggregationCatalog::with_builtins();
    let open_count = AggregationSpec::new(None, open());
    assert_eq!(computed(&catalog, "count", open_count), int(3));
    let open_sum = AggregationSpec::new(Some("amount".into()), open());
    assert_eq!(computed(&catalog, "sum", open_sum), int(24));
}

#[test]
fn require_column_hands_back_the_column() -> Result<()> {
    assert_eq!(on("amount").require_column("sum")?, "amount");
    Ok(())
}

/// Largest minus smallest of a column: a reduction this crate doesn't ship.
struct Spread(String);

impl Aggregation for Spread {
    type Output = CborValue;

    fn compute(&self, rows: &Rows, schema: &VistaMetadata) -> CborValue {
        let min = vantage_diorama::aggregate::Min::of(self.0.clone()).compute(rows, schema);
        let max = vantage_diorama::aggregate::Max::of(self.0.clone()).compute(rows, schema);
        match (min, max) {
            (CborValue::Integer(lo), CborValue::Integer(hi)) => {
                int((i128::from(hi) - i128::from(lo)) as i64)
            }
            _ => CborValue::Null,
        }
    }

    fn reads(&self) -> Reads {
        Reads::Columns(vec![self.0.clone()])
    }
}

fn with_spread() -> AggregationCatalog {
    let mut catalog = AggregationCatalog::with_builtins();
    catalog.register("spread", |op, spec| {
        Ok(Box::new(Spread(spec.require_column(op)?)) as ScalarAggregation)
    });
    catalog
}

#[test]
fn a_registered_aggregation_is_declarable_and_takes_conditions() {
    let catalog = with_spread();
    assert!(catalog.names().contains(&"spread"));
    assert_eq!(computed(&catalog, "spread", on("amount")), int(5));
    let open_spread = AggregationSpec::new(Some("amount".into()), open());
    assert_eq!(computed(&catalog, "spread", open_spread), int(3));
}

#[test]
fn registering_a_known_name_replaces_its_builder() {
    let mut catalog = AggregationCatalog::with_builtins();
    catalog.register("count", |_, _| {
        Ok(Box::new(Sum::of("amount")) as ScalarAggregation)
    });
    assert_eq!(computed(&catalog, "count", bare()), int(29));
    assert_eq!(catalog.names().len(), 6);
}

#[test]
fn a_built_aggregation_reads_what_its_reduction_and_conditions_read() {
    let catalog = AggregationCatalog::with_builtins();
    let spec = AggregationSpec::new(Some("amount".into()), open());
    let sum = catalog.build("sum", spec).expect("builds");
    let Reads::Columns(mut columns) = sum.reads() else {
        panic!("a sum reads columns: {:?}", sum.reads());
    };
    columns.sort();
    assert_eq!(columns, ["amount", "status"]);
    let plain = catalog.build("sum", on("amount")).expect("builds");
    assert_eq!(plain.reads(), Reads::Columns(vec!["amount".into()]));
}

/// A column the conditions and the reduction both read is read once.
#[test]
fn a_column_read_twice_is_listed_once() {
    let catalog = AggregationCatalog::with_builtins();
    let spec = AggregationSpec::new(
        Some("amount".into()),
        Conditions::new().eq("amount", int(7)),
    );
    let sum = catalog.build("sum", spec).expect("builds");
    assert_eq!(sum.reads(), Reads::Columns(vec!["amount".into()]));
    assert_eq!(sum.compute(&orders(), &VistaMetadata::new()), int(14));
}

/// A built aggregation is one type, so configuration can mount it as a value.
#[tokio::test]
async fn a_built_aggregation_mounts_as_a_value() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    for (id, row) in orders() {
        shell.set_record(id, row);
    }
    let dio = source(shell).await;
    let catalog = AggregationCatalog::with_builtins();
    let spec = AggregationSpec::new(Some("amount".into()), open());
    let open_total = dio
        .value_scenery(catalog.build("sum", spec)?)
        .open()
        .await?;
    settle().await;
    assert_eq!(as_i64(open_total.value()), Some(24));
    Ok(())
}

#[test]
fn group_by_carries_the_key_and_the_named_reduction_per_group() {
    let catalog = AggregationCatalog::with_builtins();
    let grouped = catalog
        .group_by("status", "sum", "total", on("amount"))
        .expect("builds");
    let out = grouped.compute(&orders(), &VistaMetadata::new());
    let columns: Vec<&str> = out.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(columns, ["status", "total"]);
    let total_of = |key: &str| {
        out.rows
            .iter()
            .find(|(_, row)| row.get("status") == Some(&text(key)))
            .and_then(|(_, row)| row.get("total").cloned())
    };
    assert_eq!(out.len(), 2);
    assert_eq!(total_of("open"), Some(int(24)));
    assert_eq!(total_of("done"), Some(int(5)));
}

#[test]
fn a_grouped_reduction_agrees_with_the_ungrouped_one_over_the_group() {
    let catalog = AggregationCatalog::with_builtins();
    let schema = VistaMetadata::new();
    let grouped = catalog
        .group_by("status", "avg", "mean", on("amount"))
        .expect("builds")
        .compute(&orders(), &schema);
    let open_mean = grouped
        .rows
        .iter()
        .find(|(_, row)| row.get("status") == Some(&text("open")))
        .and_then(|(_, row)| row.get("mean").cloned());
    let ungrouped = Where::new(open(), catalog.build("avg", on("amount")).expect("builds"))
        .compute(&orders(), &schema);
    assert_eq!(open_mean, Some(ungrouped));
}

#[test]
fn a_bad_group_by_declaration_fails_when_built() {
    let catalog = AggregationCatalog::with_builtins();
    assert!(catalog.group_by("status", "median", "m", bare()).is_err());
    assert!(catalog.group_by("status", "sum", "s", bare()).is_err());
}

#[test]
fn a_registered_aggregation_is_groupable() {
    let grouped = with_spread()
        .group_by("status", "spread", "spread", on("amount"))
        .expect("builds")
        .compute(&orders(), &VistaMetadata::new());
    let open = grouped
        .rows
        .iter()
        .find(|(_, row)| row.get("status") == Some(&text("open")));
    assert_eq!(open.and_then(|(_, row)| row.get("spread")), Some(&int(3)));
}

/// A grouped Dio built from the catalog follows its source like any aggregate.
#[tokio::test]
async fn a_catalog_group_by_mounts_as_a_derived_dio() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    for (id, row) in orders() {
        shell.set_record(id, row);
    }
    let src = source(shell.clone()).await;
    let catalog = AggregationCatalog::with_builtins();
    let grouped = catalog.group_by("status", "count", "n", bare())?;
    let derived = src.aggregate(grouped).await?;
    let rows = derived.cached().list_values().await?;
    assert_eq!(rows.get("open").and_then(|r| r.get("n")), Some(&int(3)));

    shell.set_field("b", "status", text("open"));
    src.refresh().await?;
    settle().await;
    let rows = derived.cached().list_values().await?;
    assert_eq!(rows.get("open").and_then(|r| r.get("n")), Some(&int(4)));
    assert!(rows.get("done").is_none(), "the emptied group is gone");
    Ok(())
}

#[test]
fn count_through_the_catalog_is_count() {
    let catalog = AggregationCatalog::with_builtins();
    let schema = VistaMetadata::new();
    assert_eq!(
        catalog
            .build("count", bare())
            .expect("builds")
            .compute(&orders(), &schema),
        Count::rows().compute(&orders(), &schema)
    );
}
