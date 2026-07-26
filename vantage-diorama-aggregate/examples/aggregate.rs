//! Both surfaces over one live source.
//!
//! ```console
//! cargo run -p vantage-diorama-aggregate --example aggregate
//! ```

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Lens, SortDir, TableScenery};
use vantage_diorama_aggregate::{AggregateLens, Count, GroupBy, Reduce, Sum};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn order(status: &str, amount: i64) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("status".to_string(), CborValue::Text(status.to_string()));
    record.insert("amount".to_string(), CborValue::Integer(amount.into()));
    record
}

#[tokio::main]
async fn main() -> Result<()> {
    // ---- a source Dio over a mutable in-memory master ----------------------
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("status", "String"))
        .with_column(Column::new("amount", "i64"))
        .with_id_column("id");
    let shell = MockShell::new().with_metadata(metadata);
    shell.set_record("1", order("open", 120));
    shell.set_record("2", order("open", 80));
    shell.set_record("3", order("shipped", 45));

    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .on_refresh(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().clear().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .build()
            .map_err(|e| vantage_core::error!("lens", detail = e.to_string()))?,
    );
    let orders = lens
        .make_dio(Vista::new("orders", Box::new(shell.clone())))
        .await?;

    // ---- the aggregates ----------------------------------------------------
    let aggregates = AggregateLens::in_memory()
        .debounce(Duration::from_millis(50))
        .build()?;

    let count = aggregates
        .value(&orders, "orders.count", Count::rows())
        .await?;
    let revenue = aggregates
        .value(&orders, "orders.revenue", Sum::new("amount"))
        .await?;

    let by_status = aggregates
        .derive(
            &orders,
            "orders.by_status",
            GroupBy::column(
                "status",
                Reduce::new(
                    vec![Column::new("orders", "i64"), Column::new("revenue", "i64")],
                    |_key, rows| {
                        let total: i64 = rows
                            .iter()
                            .filter_map(|r| r.get("amount"))
                            .filter_map(|v| match v {
                                CborValue::Integer(i) => Some(i128::from(*i) as i64),
                                _ => None,
                            })
                            .sum();
                        let mut out = Record::new();
                        out.insert(
                            "orders".to_string(),
                            CborValue::Integer((rows.len() as i64).into()),
                        );
                        out.insert("revenue".to_string(), CborValue::Integer(total.into()));
                        out
                    },
                ),
            ),
        )
        .await?;

    // The derived Dio is an ordinary Dio: open a scenery, sort it, read rows.
    let grid = by_status
        .table_scenery()
        .sort("revenue", SortDir::Desc)
        .open()
        .await?;

    settle().await;
    report("initial", &count, &revenue, &grid);

    // ---- the source changes ------------------------------------------------
    shell.set_record("4", order("open", 200));
    shell.set_field("3", "status", CborValue::Text("open".to_string()));
    orders.refresh().await?;
    settle().await;
    report("after the source changed", &count, &revenue, &grid);

    Ok(())
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

fn report(
    label: &str,
    count: &Arc<dyn vantage_diorama::ValueScenery>,
    revenue: &Arc<dyn vantage_diorama::ValueScenery>,
    grid: &Arc<dyn TableScenery>,
) {
    println!("\n{label}");
    println!("  orders: {:?}", scalar(count));
    println!("  revenue: {:?}", scalar(revenue));
    for index in 0..grid.row_count() {
        let Some(row) = grid.row(index) else { continue };
        println!(
            "  {:<10} {:>3} orders  {:>5}",
            text(&row.record, "status"),
            number(&row.record, "orders"),
            number(&row.record, "revenue"),
        );
    }
}

fn scalar(scenery: &Arc<dyn vantage_diorama::ValueScenery>) -> i64 {
    match scenery.value() {
        Some(CborValue::Integer(i)) => i128::from(i) as i64,
        _ => 0,
    }
}

fn text(record: &Record<CborValue>, field: &str) -> String {
    match record.get(field) {
        Some(CborValue::Text(s)) => s.clone(),
        _ => String::new(),
    }
}

fn number(record: &Record<CborValue>, field: &str) -> i64 {
    match record.get(field) {
        Some(CborValue::Integer(i)) => i128::from(*i) as i64,
        _ => 0,
    }
}
