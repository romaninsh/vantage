mod support;
use ciborium::Value as CborValue;
use support::aggregate::{as_i64, int, master, record, settle, source, text};
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::aggregate::{Conditions, Count, GroupBy, Reduce, Sum, Where};
use vantage_types::Record;
use vantage_vista::Column;

#[tokio::test]
async fn many_independent_aggregations_over_one_dio() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    shell.set_record(
        "a",
        record(&[("status", text("open")), ("amount", int(10))]),
    );
    shell.set_record("b", record(&[("status", text("done")), ("amount", int(5))]));
    let src = source(shell.clone()).await;

    // Three values and a derived Dio, all on one Dio.
    let count = src.value_scenery(Count::rows()).open().await?;
    let total = src.value_scenery(Sum::of("amount")).open().await?;
    let open = src
        .value_scenery(Where::new(
            Conditions::new().eq("status", text("open")),
            Count::rows(),
        ))
        .open()
        .await?;
    let group = src
        .aggregate(GroupBy::column(
            "status",
            Reduce::new(vec![Column::new("n", "i64")], |_k, rows, _schema| {
                let mut o = Record::new();
                o.insert("n".into(), CborValue::Integer((rows.len() as i64).into()));
                o
            }),
        ))
        .await?;
    settle().await;

    assert_eq!(as_i64(count.value()), Some(2), "count");
    assert_eq!(as_i64(total.value()), Some(15), "sum");
    assert_eq!(as_i64(open.value()), Some(1), "filtered count");
    assert_eq!(group.cached().list_values().await?.len(), 2, "groups");

    // One source change must move all four, independently.
    shell.set_record("c", record(&[("status", text("open")), ("amount", int(7))]));
    src.refresh().await?;
    settle().await;

    assert_eq!(as_i64(count.value()), Some(3), "count after");
    assert_eq!(as_i64(total.value()), Some(22), "sum after");
    assert_eq!(as_i64(open.value()), Some(2), "filtered after");
    let g = group.cached().list_values().await?;
    assert_eq!(
        g.get("open").and_then(|r| r.get("n")),
        Some(&int(2)),
        "group after"
    );

    // Dropping one must not disturb the others.
    drop(total);
    shell.set_record("d", record(&[("status", text("done")), ("amount", int(1))]));
    src.refresh().await?;
    settle().await;
    assert_eq!(
        as_i64(count.value()),
        Some(4),
        "survivor still tracking after a sibling dropped"
    );
    assert_eq!(as_i64(open.value()), Some(2), "other survivor unaffected");
    Ok(())
}
