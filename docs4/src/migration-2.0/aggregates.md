# Aggregates

1.x had two aggregate vocabularies: diorama's own (`value_scenery().count()`, `.sum(c)`,
`CustomAggregate`) and the vantage-diorama-aggregate crate's (`Count`, `Sum::new`, `CountWhere`,
`AggregateLens`, `DerivedDio`), with a second Lens that persisted every value in its own cache
table. 2.0 keeps one vocabulary, folded into `vantage_diorama::aggregate` (decision 6, no shim). A
value scenery takes an `Aggregation`; a derived table comes from `dio.aggregate(..)` and is a plain
`Dio`. Nothing is persisted: the source's cache is the warm start, and a reopen recomputes. Read
[Scenery: Reactive Views](../intro/step7-scenery.md) for value sceneries in use.

The snippets run over a Dio of `teams` (rows `a` red, `b` blue).

## The crate is a module

Drop the `vantage-diorama-aggregate` dependency and import from `vantage_diorama::aggregate`.
`value_scenery` takes the aggregation, and the column aggregates are built with `of`.

```rust,compile_fail,E0432
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
use vantage_diorama_aggregate::{Conditions, Count, Max, Where};

let last = dio.value_scenery(Max::new("name")).open().await?;
let red = dio
    .value_scenery(Where::new(Conditions::new().eq("name", "red"), Count::rows()))
    .open()
    .await?;
# Ok(())
# }
```

```rust
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
use vantage_diorama::aggregate::{Conditions, Count, Max, Where};

let last = dio.value_scenery(Max::of("name")).open().await?;
let red = dio
    .value_scenery(Where::new(Conditions::new().eq("name", "red"), Count::rows()))
    .open()
    .await?;
# let _ = (last.value(), red.value());
# Ok(())
# }
```

A log filter on the old crate's target moves too: `RUST_LOG=vantage_diorama_aggregate=debug` is
`RUST_LOG=vantage_diorama::aggregate=debug`, and `vantage_diorama=warn` now covers aggregate
warnings.

## A derived table is a Dio

`AggregateLens::in_memory().build()?.derive(&src, name, agg)` becomes `src.aggregate(agg).await?`,
and `derive_value(&src, name, alias, agg)` becomes `src.aggregate(AsRows::new(alias, agg))`. The
returned `Dio` is the handle; `DerivedDio`, its `dio()` and its `Deref` are gone. Dropping every
clone stops the recomputation.

```rust,compile_fail,E0433
# use vantage_diorama::aggregate::{AsRows, Count};
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
let aggregates = AggregateLens::in_memory().build()?;
let counted = aggregates.derive(&dio, "teams.count", AsRows::new("n", Count::rows())).await?;
# Ok(())
# }
```

```rust
# use vantage_diorama::aggregate::{AsRows, Count};
# use vantage_diorama::{prelude::*, Cache, Loader, LoaderCx};
# use vantage_types::Record;
# use vantage_vista::{mocks::MockShell, prelude::*, Column, VistaMetadata};
# fn row(n: &str) -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() }
# fn teams() -> Vista {
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# async fn copy(cx: LoaderCx) -> vantage_core::Result<()> { let rows = cx.master().list_values().await?; cx.reconcile(rows).await }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let dio = Lens::builder().cache(Cache::memory()).loader(Loader::eager(copy)).build()?.dio(teams()).build().await?;
let counted = dio.aggregate(AsRows::new("n", Count::rows())).await?;
let table = counted.cached(); // a Vista like any other
# let _ = table;
# Ok(())
# }
```

Every aggregate of one source is named `<source>#aggregate` in the debug tap and diagnostics,
whatever the aggregation, so two aggregates of one source can't be told apart by name. They are
still two Dios on two private Lenses and never share a cache.

## A custom aggregation

`value_scenery().custom(f)`, `CustomAggregate` and `boxed_custom_aggregate` become an `impl
Aggregation`. The trait gained the schema on `compute` and a required `reads()`, which tells the
Dio what the computation reads: `Reads::Count` (the row count only), `Reads::Columns(..)` or
`Reads::Rows`. `GroupReducer::reduce` and the `Reduce` closure gain the schema the same way, and
`GroupReducer` gains `reads()`.

```rust
use vantage_diorama::aggregate::{Aggregation, Reads, Rows};
use vantage_vista::{CborValue, VistaMetadata};

/// The length of the longest `name`.
struct Longest;

impl Aggregation for Longest {
    type Output = CborValue;

    fn compute(&self, rows: &Rows, _schema: &VistaMetadata) -> CborValue {
        let longest = rows.values().filter_map(|r| r.get("name")?.as_text().map(str::len)).max();
        longest.map_or(CborValue::Null, |n| CborValue::from(n as i64))
    }

    fn reads(&self) -> Reads {
        Reads::Columns(vec!["name".to_string()])
    }
}
# let _ = Longest;
```

`AggregateOutput` is sealed: an aggregation returns `CborValue` (a scalar, for value sceneries) or
`DerivedRows` (a table, for `dio.aggregate`).

## `Where` compares by the column's type

`Where` and the column reductions convert an operand by the column's declared type, and compare an
untyped column as given. 1.x fell back to comparing rendered text, so a schemaless source loses
that fallback (VAN-384).

| Condition | Row | 1.x | 2.0 |
|---|---|---|---|
| `.eq("code", "200")` | `{code: 200}`, column untyped | matched | no match: declare `type: i64`, or write `.eq("code", 200)` |
| `.eq("code", "200")` | `{code: 200}`, column `i64` | matched | matches (typed columns coerce both ways, bool included) |
| `.eq("price", "9.99")` | a tag-10 decimal cell | matched | no match: write the operand in the cell's type |
| `.eq("id", "user:1")` | a record-id cell | matched | no match |
| `.eq("a.b", x)` | `{a: {b: x}}` | matched the path | matches the path; a literal `"a.b"` key only when the schema declares `a.b` |

A derived `GroupBy` key column takes the source key's type (it was always `"String"`), and is
untyped when the source key has none.

## Renames

| 1.x | 2.0 |
|---|---|
| `value_scenery().count()`, `.sum(c)`, `.max(c)`, `.min(c)` | `value_scenery(Count::rows())`, `Sum::of(c)`, `Max::of(c)`, `Min::of(c)` |
| `value_scenery().count_where(pairs)`, `CountWhere::new(conds)` | `value_scenery(Where::new(Conditions::new().eq(..), Count::rows()))` |
| `Sum::new`, `Avg::new`, `Min::new`, `Max::new`, `Distinct::new` | `Sum::of` and so on |
| `AggregateLens::value(&src, name, agg)` | `src.value_scenery(agg).open().await?` |
| `AggregateLens::{at, in_memory, with_backend, retain, close}`, `AggregateLensBuilder` | removed: no persistence |
| `AggregateLensBuilder::debounce(d)` | `Tuning::default().with_aggregate_debounce(d)` on the source's Lens |
| `AggregateValue`, `seed_from_cache`, the per-aggregate cache table | none; a reopen recomputes from the source's cache |
| `Aggregation::compute(rows)` | `compute(rows, schema)` + `reads()` |
| `GroupReducer::reduce(key, rows)`, `Reduce::new(\|k, rows\| ..)` | `reduce(key, rows, schema)` + `reads()`, `Reduce::new(columns, \|k, rows, schema\| ..)` |
| `AggregationCatalog::group_by` → `GroupBy<Reduce<impl Fn>>` | `GroupBy<GroupReduction>` |
| `Conditions::matches(&row)`, `narrow(rows)` | `matches(&row, &schema)`, `narrow(rows, &schema)` |
| `AggregationSpec { column, conditions }` | `AggregationSpec::new(column, conditions)` |
| core `Aggregate`, `CustomAggregateFuture` | an `impl Aggregation` |

## Behaviour notes

- **An aggregate over a derived Dio counts only the rows its narrowing admits**: it reads the
  source through `cached()`, residual included.
- **A recomputation writes only the rows whose value changed** (through `Dio::reconcile`), and
  still publishes one `DatasetChanged`.
- **`Reads::Rows` on an augmented Dio hydrates every cached row** on each recompute; prefer
  `Reads::Columns` when the computation names its columns.

## Checklist

- [ ] no `vantage_diorama_aggregate` import or dependency
- [ ] every `value_scenery()` call takes an aggregation; column aggregates use `of`
- [ ] no `AggregateLens`, `DerivedDio` or `derive_value`: `src.aggregate(..)` returns a `Dio`
- [ ] custom aggregations take the schema and implement `reads()`
- [ ] `Where` conditions over untyped columns either declare the column type or write the operand
      in the cell's own type
