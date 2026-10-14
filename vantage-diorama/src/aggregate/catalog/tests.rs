use super::*;
use vantage_types::Record;
use vantage_vista::Column;

fn rows() -> Rows {
    let mut a = Record::new();
    a.insert("Event".into(), CborValue::Text("opened".into()));
    a.insert("Size".into(), CborValue::Integer(10.into()));
    let mut b = Record::new();
    b.insert("Event".into(), CborValue::Text("failed".into()));
    b.insert("Size".into(), CborValue::Integer(20.into()));
    [("a".to_string(), a), ("b".to_string(), b)]
        .into_iter()
        .collect()
}

fn spec(column: Option<&str>, conditions: Conditions) -> AggregationSpec {
    AggregationSpec {
        column: column.map(str::to_string),
        conditions,
    }
}

fn schema() -> VistaMetadata {
    VistaMetadata::new()
}

#[test]
fn builds_a_named_reduction() {
    let catalog = AggregationCatalog::with_builtins();
    let agg = catalog
        .build("sum", spec(Some("Size"), Conditions::new()))
        .unwrap();
    assert_eq!(agg.compute(&rows(), &schema()), CborValue::Integer(30.into()));
}

/// Conditions are applied by the catalog, so they work for every name
/// including ones registered from outside this crate.
#[test]
fn applies_conditions_to_any_named_reduction() {
    let catalog = AggregationCatalog::with_builtins();
    let agg = catalog
        .build(
            "sum",
            spec(
                Some("Size"),
                Conditions::new().eq("Event", CborValue::Text("opened".into())),
            ),
        )
        .unwrap();
    assert_eq!(agg.compute(&rows(), &schema()), CborValue::Integer(10.into()));
}

#[test]
fn an_unknown_name_lists_what_is_available() {
    let catalog = AggregationCatalog::with_builtins();
    // `unwrap_err` would need `Debug` on a boxed trait object; match instead.
    let err = match catalog.build("median", spec(Some("Size"), Conditions::new())) {
        Ok(_) => panic!("an unknown op must not build"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("median"), "names the bad op: {err}");
    assert!(err.contains("distinct"), "lists the known ops: {err}");
}

/// `count` is the only builtin that reduces whole rows; the rest must say
/// what they reduce, and saying so badly is an error, not a zero.
#[test]
fn a_reduction_without_its_column_fails_loudly() {
    let catalog = AggregationCatalog::with_builtins();
    assert!(catalog.build("sum", spec(None, Conditions::new())).is_err());
    assert!(
        catalog
            .build("count", spec(None, Conditions::new()))
            .is_ok()
    );
}

#[test]
fn a_registered_aggregation_becomes_declarable() {
    let mut catalog = AggregationCatalog::with_builtins();
    catalog.register("always_seven", |_, _| {
        Ok(Box::new(Sevens) as ScalarAggregation)
    });
    let agg = catalog
        .build("always_seven", spec(None, Conditions::new()))
        .unwrap();
    assert_eq!(agg.compute(&rows(), &schema()), CborValue::Integer(7.into()));
}

/// Grouping routes through the same catalog as a top-level reduction, so
/// the groups must add up to the whole. If these ever diverge, two copies
/// of "sum" have appeared.
#[test]
fn the_groups_agree_with_the_ungrouped_reduction() {
    let catalog = AggregationCatalog::with_builtins();
    let grouped = catalog
        .group_by(
            "Event",
            "sum",
            "bytes",
            spec(Some("Size"), Conditions::new()),
        )
        .unwrap()
        .compute(&rows(), &schema());
    let total = catalog
        .build("sum", spec(Some("Size"), Conditions::new()))
        .unwrap()
        .compute(&rows(), &schema());

    assert_eq!(grouped.len(), 2, "one row per distinct Event");
    let summed: i64 = grouped
        .rows
        .iter()
        .filter_map(|(_, row)| match row.get("bytes") {
            Some(CborValue::Integer(i)) => Some(i128::from(*i) as i64),
            _ => None,
        })
        .sum();
    assert_eq!(CborValue::Integer(summed.into()), total);
}

/// The key column comes back on every row — a breakdown you can't label
/// isn't one.
#[test]
fn every_group_carries_its_key() {
    let catalog = AggregationCatalog::with_builtins();
    let grouped = catalog
        .group_by("Event", "count", "n", spec(None, Conditions::new()))
        .unwrap()
        .compute(&rows(), &schema());
    for (_, row) in &grouped.rows {
        assert!(row.get("Event").is_some(), "row without its key: {row:?}");
        assert!(row.get("n").is_some());
    }
}

/// A declared group's conditions see the source schema, so a YAML
/// `Size: "10"` coerces inside a group as it does at the top level.
#[test]
fn a_grouped_where_coerces_by_the_source_schema() {
    let catalog = AggregationCatalog::with_builtins();
    let typed = VistaMetadata::new().with_column(Column::new("Size", "i64"));
    let grouped = catalog
        .group_by(
            "Event",
            "count",
            "n",
            spec(None, Conditions::new().eq("Size", CborValue::Text("10".into()))),
        )
        .unwrap()
        .compute(&rows(), &typed);
    let opened = grouped.rows.iter().find(|(id, _)| id == "opened").unwrap();
    assert_eq!(opened.1.get("n"), Some(&CborValue::Integer(1.into())));
}

/// A declared group reads its key and what the built reduction reads, not
/// every column, so an augmented source hydrates only those.
#[test]
fn a_declared_group_reads_its_key_and_the_reductions_column() {
    let catalog = AggregationCatalog::with_builtins();
    let grouped = catalog
        .group_by(
            "Event",
            "sum",
            "bytes",
            spec(Some("Size"), Conditions::new()),
        )
        .unwrap();
    assert_eq!(
        grouped.reads(),
        Reads::Columns(vec!["Event".to_string(), "Size".to_string()])
    );
}

struct Sevens;
impl Aggregation for Sevens {
    type Output = CborValue;

    fn reads(&self) -> Reads {
        Reads::Columns(Vec::new())
    }

    fn compute(&self, _rows: &Rows, _schema: &VistaMetadata) -> CborValue {
        CborValue::Integer(7.into())
    }
}
