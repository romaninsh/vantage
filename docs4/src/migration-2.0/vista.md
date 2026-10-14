# Vista

`Vista` is the type-erased `Table<T, E>`, and 2.0 makes it behave like one: it is `Clone`, owns its
query, narrows with consuming `with_*` and changes in place only through `add_*` / `set_*` on a
value you own. The `TableShell` behind it lost about twenty of its 44 methods. The ones that went
were variants (`fetch_page` beside `fetch_window`), silent defaults (a `can_*` flag the shell never
checked), or reads of the shell's own state that `metadata()` now answers. Rule 3 drove the
capability change: each `Capability` names one method family, and a contract test calls every one a
shell advertises. The chapter to read alongside this page is
[Vista: the Universal Data Handle](../intro/step4-vista.md).

The snippets below run against an in-memory `MockShell` named `teams`, with rows `a` (red) and `b`
(blue).

## One read primitive

`fetch_page`, `fetch_next`, `set_page_size` and `fetch_window_counted` fold into
`fetch_window(offset, limit)`, which returns a `Page` carrying the rows and, when the shell knows
it, the total. Cursor paging is `fetch_cursor(token, limit)`.

```rust,compile_fail,E0599
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let vista = teams();
let rows = vista.fetch_page(0, 10).await?;
# Ok(())
# }
```

```rust
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let vista = teams();
let page = vista.fetch_window(0, 10).await?;
let (rows, total) = (page.rows, page.total);
# assert_eq!(rows.len(), 2);
# let _ = total;
# Ok(())
# }
```

Every shell whose `list` works must answer `fetch_window(0, n)`; `Window` now means "any offset",
and `Vista` refuses a later offset without it before the shell is called.

## One condition method

`add_eq_condition`, `add_op_condition` and `add_condition_eq` are one `add_condition(column, op,
value)` on an owned Vista, or `with_condition` to narrow a copy.

```rust,compile_fail,E0599
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let mut vista = teams();
vista.add_eq_condition("name", "red")?;
# Ok(())
# }
```

```rust
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let red = teams().with_condition("name", FilterOp::Eq, "red")?;
# assert_eq!(red.list_values().await?.len(), 1);
# Ok(())
# }
```

## Search and order are set, not added

`add_search` / `clear_search` and `add_order` / `clear_order` become `set_search(Option)` and
`set_order(Option)`: `None` clears. A Vista holds one search and one order, so "add" described
something the shells never did.

```rust,compile_fail,E0599
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let mut vista = teams();
vista.add_order("name", SortDirection::Ascending)?;
# Ok(())
# }
```

```rust
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String").with_flag("orderable")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let mut vista = teams();
vista.set_order(Some(("name", SortDirection::Ascending)))?;
vista.set_search(None)?;
# assert!(vista.query().order.is_some());
# Ok(())
# }
```

## A query in, a residual out

`with_query` (strict) and `with_query_partial` are one `with_query(&query)`, which returns the
narrowed Vista and the residual: the clauses the shell didn't take. Nothing is dropped silently;
the caller evaluates the residual, or refuses it.

```rust,compile_fail,E0599
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let query = VistaQuery::new().with_condition(Condition::new("name", FilterOp::Eq, "red"));
let (narrowed, residual) = teams().with_query_partial(&query)?;
# Ok(())
# }
```

```rust
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let query = VistaQuery::new().with_condition(Condition::new("name", FilterOp::Eq, "red"));
let (narrowed, residual) = teams().with_query(&query)?;
# assert!(residual.is_empty());
# let _ = narrowed;
# Ok(())
# }
```

## Capabilities are a typed set

The `can_*` flags become `vista.capabilities().has(Capability::…)`. `can_fetch_page`,
`can_fetch_next` and `can_set_page_size` map to `Window` and `Cursor`; the traversal flags map to
`Traverse`; `can_count` and `can_invalidate` have no replacement, because `count` always works and
nothing invalidates a Vista.

```rust,compile_fail,E0599
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let paged = teams().can_fetch_page();
# Ok(())
# }
```

```rust
# use vantage_types::Record;
# use vantage_vista::mocks::MockShell;
# use vantage_vista::prelude::*;
# use vantage_vista::{Column, VistaMetadata};
# fn teams() -> Vista {
#     let row = |n: &str| -> Record<CborValue> { [("name".to_string(), CborValue::Text(n.into()))].into_iter().collect() };
#     let meta = VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id")).with_column(Column::new("name", "String")).with_id_column("id");
#     Vista::new("teams", MockShell::new().with_metadata(meta).with_record("a", row("red")).with_record("b", row("blue")))
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let paged = teams().capabilities().has(Capability::Window);
# assert!(paged);
# Ok(())
# }
```

## Renames

| 1.x | 2.0 |
|---|---|
| `get_column_names()`, `get_column(n)`, `get_id_column()`, `get_title_columns()` | `columns().keys()`, `column(n)`, `id_column()`, `title_columns()` |
| `get_references()`, `list_references()`, `get_reference(n)` | `references()`, `reference(n)` |
| `driver()`, `has_auto_id()`, `can_watch()` | `driver_name()`, the id column's `ColumnFlag::Auto`, `capabilities().has(Capability::Subscribe)` |
| `preview_query()` (JSON with `driver`) | `preview()` → `Preview { query, note }`; the driver is `driver_name()` |
| `stream_vista_values`, `Vista::stream_values` | `list_values()` |
| `get_vista_value_with_row`, `Vista::get_value_with_row` | `get_value(id)`; a row hint goes to an augmentation's `Fetch::Custom` |
| shell `get_vista_some_value` | `fetch_window(0, 1)`; `Vista::get_some_value` reads exactly that |
| shell `insert_vista_return_id_value` | `TableShell::insert(None, record)` |
| `get_ref_target`, `get_contained_ref`, `contained()`, `get_ref_kinds`, `list_contained` | `get_ref(relation, Option<&row>)`; the kind is `reference(n)?.kind` |
| `aggregate_vista`, `Vista::aggregate`, `AggregateSpec` | removed; see [Aggregates](./aggregates.md) |
| `with_id(&mut self)`, `set_name` | `with_id(self) -> Result<Vista>`, `with_name(self)` |
| `index_key`, the `narrowed` flag, `pub source` | `query().key()`, `is_narrowed()`, `map_shell` |
| `EmptyShell`, `Vista::empty` | `vantage_vista::mocks::MockShell` (the `mocks` feature) |
| `Column.original_type: String` | `Option<String>`; `Column::untyped(name)` has no type |
| `AnyExpression`, `ExpressionLike` | `vantage-expressions` |

## For shell authors

| 1.x `TableShell` | 2.0 |
|---|---|
| `clone_shell` | the `Clone` supertrait (`CloneTableShell`) |
| `columns()`, `references()`, `id_column()` | one `metadata() -> &VistaMetadata` |
| a `&Vista` parameter on shell methods | gone |
| `add_raw_condition` | `add_native_condition(NativeCondition)` |
| `add_search` + `clear_search`, `add_order` + `clear_orders` | `set_search(Option<&str>)`, `set_order(Option<(&str, SortDirection)>)` |
| `capability_flag`, `default_error` | `capabilities()`, `unsupported(Capability::…)` |
| `fetch_page`, `fetch_next`, slicing a listing | `fetch_window -> Page`, `fetch_cursor`, `Page::from_listing` ([Drivers](./drivers.md)) |
| `Preview::new(driver)` | `Preview::new()` |
| `get_ref` required | defaults to `unsupported(Capability::Traverse)` |
| a refusal for a dotted column | `condition_column_error(&metadata, column)` |
| struct literals of `VistaSpec`, `ColumnSpec`, `ReferenceSpec`, `ContainedYaml`, `NoExtras` | `VistaSpec::new(name)`, `ColumnSpec::new()`, `NoExtras::default()` |
| `resolve_id_column()`, `resolve_id_column_or(d)`, `foreign_key_or(r)` | `resolve_id_column(default)`, `foreign_key(relation)` |
| `VistaMetadata::with_column_at`, `ContainedShell`, `refuse_path` | `with_columns_at`; crate-private; `condition_column_error` |
| `eval::path_get(row, "a.b")` | `eval::cell_get(row, "a.b", &metadata)` |
| `contract::check_leading_window`, `Preview::default()` | run by `contract::check_advertised`; `Preview::new()` |

## Behaviour notes

- **One in-memory evaluator.** `vantage_vista::eval` (`Condition::matches`, `VistaQuery::matches`,
  `VistaQuery::sort_rows`, `cmp_rows`, `cell_get`, `cbor_cmp`) is what `MockShell`, the facade and
  every local refinement use. Search is a case-insensitive substring of any cell's text, numbers,
  bools, tagged values and nested cells included; a blank needle matches every row. Null and absent
  cells sort last in both directions, ties break on id, and `3 == 3.0`.
- **Dotted paths.** A condition on `root.leaf` that a shell can't evaluate is refused as
  `Unimplemented` ("this shell doesn't evaluate a dotted path"); `with_query` hands it back in the
  residual. Test `is_unimplemented()` rather than the message. An unknown column reads "Unknown
  column for condition" with a `field` key on every shell, and the contract's dotted-path probe
  fails a shell that answers anything but a narrowing or that refusal.
- **`MockShell`** gained `calls()` (`MockCalls`, shared by clones), `get_only()`, `gated(..)`,
  `get_log()` and `max_concurrent_gets()`; replace hand-rolled counting and get-only test shells.
- **`get_some_value` reads `fetch_window(0, 1)` only**, with no fallback to `list()`.
- **A contained relation without a row**: `get_ref(rel, None)` returns the bare target, with no
  records and no write capabilities.
- **Untyped columns.** A YAML column without `type:` is `Column::untyped`; rhai `columns()` reports
  its type as `()`, and the rhai `new_id` gives it a UUIDv7. A numeric id column the rhai insert
  can't fill (`u128`, `usize`, `Option<i64>`) now errors unless it is flagged `auto` or an id is
  passed.
- **Quicksearch reads `Searchable` columns**, or every column when none is flagged. See
  [Drivers](./drivers.md#quicksearch-reads-the-searchable-columns).

## Checklist

- [ ] no `fetch_page`, `fetch_next` or `set_page_size`; windows are `fetch_window` → `Page`
- [ ] every condition is `add_condition` / `with_condition` with a `FilterOp`
- [ ] `set_search` / `set_order` take an `Option`; `None` clears
- [ ] `with_query` callers handle the residual it returns
- [ ] no `can_*` flags; `capabilities().has(Capability::…)`
- [ ] metadata reads use the short names in the table above
- [ ] a custom `TableShell` implements `metadata()`, derives `Clone`, and refuses with
      `unsupported(..)` / `condition_column_error(..)`
