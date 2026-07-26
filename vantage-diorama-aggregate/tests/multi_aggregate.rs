mod support;
use ciborium::Value as CborValue;
use support::{DEBOUNCE, int, master, record, settle, source, text};
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama_aggregate::{AggregateLens, Count, CountWhere, GroupBy, Reduce, Sum};
use vantage_types::Record;
use vantage_vista::Column;

fn i(v: Option<CborValue>) -> Option<i64> {
    match v {
        Some(CborValue::Integer(x)) => Some(i128::from(x) as i64),
        _ => None,
    }
}

#[tokio::test]
async fn many_independent_aggregations_over_one_dio() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    shell.set_record(
        "a",
        record(&[("status", text("open")), ("amount", int(10))]),
    );
    shell.set_record("b", record(&[("status", text("done")), ("amount", int(5))]));
    let src = source(shell.clone()).await;

    // Two different lenses AND two aggregates on one lens, all on one Dio.
    let l1 = AggregateLens::in_memory().debounce(DEBOUNCE).build()?;
    let l2 = AggregateLens::in_memory().debounce(DEBOUNCE).build()?;

    let count = l1.value(&src, "count", Count::rows()).await?;
    let total = l1.value(&src, "total", Sum::new("amount")).await?;
    let open = l2
        .value(&src, "open", CountWhere::new().eq("status", text("open")))
        .await?;
    let group = l2
        .derive(
            &src,
            "byst",
            GroupBy::column(
                "status",
                Reduce::new(vec![Column::new("n", "i64")], |_k, rows| {
                    let mut o = Record::new();
                    o.insert("n".into(), CborValue::Integer((rows.len() as i64).into()));
                    o
                }),
            ),
        )
        .await?;
    settle().await;

    assert_eq!(i(count.value()), Some(2), "count");
    assert_eq!(i(total.value()), Some(15), "sum");
    assert_eq!(i(open.value()), Some(1), "filtered count");
    assert_eq!(group.vista().list_values().await?.len(), 2, "groups");

    // One source change must move all four, independently.
    shell.set_record("c", record(&[("status", text("open")), ("amount", int(7))]));
    src.refresh().await?;
    settle().await;

    assert_eq!(i(count.value()), Some(3), "count after");
    assert_eq!(i(total.value()), Some(22), "sum after");
    assert_eq!(i(open.value()), Some(2), "filtered after");
    let g = group.vista().list_values().await?;
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
        i(count.value()),
        Some(4),
        "survivor still tracking after a sibling dropped"
    );
    assert_eq!(i(open.value()), Some(2), "other survivor unaffected");
    Ok(())
}
