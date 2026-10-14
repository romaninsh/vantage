# vantage-diorama-aggregate

Client-side aggregation over a [`vantage-diorama`](../vantage-diorama) `Dio`.
It derives counts, totals and grouped tables from the Dio's local copy and
keeps them current as the source changes.

```toml
[dependencies]
vantage-diorama-aggregate = "0.6.10"
```

```rust,ignore
use vantage_diorama::SortDir;
use vantage_diorama_aggregate::{AggregateLens, Count, CountWhere, GroupBy, Sum};

let agg = AggregateLens::at("aggregates.redb")?.build()?;

// Numbers, mounted as a ValueScenery.
let all = agg.value(&orders, "orders.all", Count::rows()).await?;
let open = agg
    .value(&orders, "orders.open", CountWhere::new().eq("status", "open"))
    .await?;
let total = agg.value(&orders, "orders.total", Sum::new("amount")).await?;
println!("{:?}", total.value());

// A grouped table, mounted as its own Dio.
let by_status = agg
    .derive(&orders, "orders.by_status", GroupBy::column("status", revenue))
    .await?;
let grid = by_status.table_scenery().sort("revenue", SortDir::Desc).open().await?;
```

`revenue` is a `Reduce`: the output columns plus a closure that turns one
group's rows into one record (see `tests/aggregate.rs`).

## Main types

- `AggregateLens`: the cache for all aggregates, one table per aggregate name.
  `at(path)` uses a redb file, `in_memory()` keeps nothing on disk, and
  `with_backend(..)` shares a backend with a datasource. The builder takes
  `debounce(..)` (default 250 ms).
- `value(..)` returns an `Arc<dyn ValueScenery>` (backed by `AggregateValue`). `derive(..)`
  returns a `DerivedDio`, an ordinary Dio with its own Vista that supports
  sort, search and count. `derive_value(..)` puts a scalar into a one-row Dio
  so it binds like any other table.
- Built-in reductions: `Count`, `CountWhere`, `Sum`, `Avg`, `Min`, `Max`,
  `Distinct`, and `GroupBy` with a `Reduce` per group. `Where` narrows any
  aggregation by `Conditions`.
- `AggregationCatalog::with_builtins()` builds aggregations by name (`count`,
  `sum`, `avg`, `min`, `max`, `distinct`) from an `AggregationSpec`, for
  config-driven setups. `register` adds your own.

## Design

Every change recomputes the whole aggregation. There are no accumulators, so
nothing drifts and `min`, `max` and `distinct` need no special handling. The
engine debounces bursts and compares each output with the last one before
publishing, so recomputing often does not mean repainting often. The cache
gives a warm start after a restart; the value is recomputed behind it.
Call `close()` before exit, and `retain(&[..])` to drop aggregates you no
longer use.
