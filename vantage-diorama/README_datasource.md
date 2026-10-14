# Implementing a Data Source

A source reaches Diorama in two halves. The **driver** is a `TableShell`
behind a `Vista`: it says what the backend can do and does it. The
**loader** is the Lens's side: it decides how the driver's rows get into a
Dio's cache. This guide covers what each half owes the other, and the one
rule Diorama holds both to.

If you haven't written a Vista driver yet, start with the persistence
walkthrough in the book and the `TableShell` docs in `vantage-vista`. This
guide picks up where they leave off.

## The driver: be honest

There is no Diorama trait to implement. A driver that implements
`TableShell` and advertises its capabilities truthfully is already a good
Diorama source. Diorama wraps it, caches it and asks it hard questions; the
driver's whole job is to answer them accurately.

### Capabilities

`VistaCapabilities` is a set of `Capability` values. Each capability names
one method family, and a shell that advertises it must answer that family
without refusing:

| Capability | What Diorama does with it |
|---|---|
| `Window` | A table view pages windows at any offset through a paged loader. Without it, the loader copies the set whole. |
| `Cursor` | An eager start can copy the set page by page and resume. |
| `Order`, `Search`, `FilterOps` | A view's order, search and non-equality conditions are pushed to the backend. Without them, Diorama evaluates them over a complete set. |
| `Subscribe` | `.live()` on the Dio builder follows `watch()`. Without it, the refresher keeps the cache fresh. |
| `Insert`, `Update`, `Delete` | A flash goes through to the master. Without them, the Dio is read-only unless the Lens has a writer. |

`list`, `get`, `count`, equality conditions and the leading window
(`fetch_window(0, n)`) carry no capability: every shell answers them.

```rust
let shell = MockShell::new()
    .with_metadata(metadata)
    .with_capabilities(VistaCapabilities::new().with(Capability::Window));
```

### Refuse, don't reject

A method can fail two ways, and Diorama treats them differently:

- **Refused**: an `Unsupported` (or `Unimplemented`) error. "I can't do
  this here." `Vista::with_query` hands the clause back to the caller, and
  Diorama evaluates it itself.
- **Rejected**: any other error. "This is wrong." An unknown column, a value
  the backend can't express. The view fails, naming the clause.

The REST shell in `vantage-api-client` shows both in one method, trimmed
here to the two decisions:

```rust
fn add_condition(&mut self, condition: &Condition) -> Result<()> {
    let field = condition.column.as_str();
    if !self.metadata.columns.contains_key(field) {
        return Err(condition_column_error(&self.metadata, field));
    }
    if condition.op != FilterOp::Eq {
        return unsupported(Capability::FilterOps);
    }
    self.table.add_condition(crate::eq_condition(field, condition.value.clone()));
    Ok(())
}
```

An unknown column is rejected, so a typo never turns into a silent local
filter. An operator the API has no query parameter for is refused, so
Diorama filters on it instead. `condition_column_error` refuses a dotted
path into a declared column (Diorama can evaluate `dims.w` over the rows),
and rejects everything else.

Refusing per call is how a backend that sorts only on indexed columns
advertises `Order` and still tells the truth: `set_order` refuses the
unindexed ones.

### Totals

A window read returns a `Page { rows, total }`. When the backend knows the
grand total of the narrowed set, put it in `total` (`Page::with_total`); a
paged view sizes its scrollbar from it before the rows load. When the
backend doesn't know, leave it `None`. A guessed total is worse than none:
the view believes it.

### Change streams

Advertise `Subscribe` only when `watch()` pushes with no further setup. A
stream that stays silent can't be told apart from nothing happening, so a
driver that needs the application to configure something first doesn't
advertise it until the application has.

Granularity is up to the backend. Per-row `VistaChange::Updated { id, value }`
lets Diorama update one cache slot; `VistaChange::Invalidated` makes it
re-read the set. Both are correct; the first is cheaper.

A backend with no change stream leaves `Subscribe` off. Polling a REST API
and diffing it into synthetic events is a policy (how often, how to dedupe,
how to spot deletes), and policy belongs in the Lens's refresher, where the
application can change it.

### Contract checks

`vantage_vista::contract` (feature `contract`) probes every advertised
capability and fails, listing every broken rule, when one isn't backed by a
working method:

```rust
#[tokio::test]
async fn csv_contract_advertised() {
    contract::check_advertised(&clients()).await;
}
```

Run it against your own fixture. It is the cheapest way to find out that a
capability you advertise is one Diorama will lean on and your driver can't
deliver.

## The loader: fill the cache

The loader is set on the Lens, and it is where a source's shape meets the
cache. `README_lens.md` covers the Lens as a whole; this is the source-facing
part.

### Eager

`Loader::eager(start)` copies the whole set once, when the Dio is built. The
start receives a `LoaderCx` and either hands the set to `cx.reconcile(rows)`
or writes `cx.cache()` directly:

```rust
let loader = Loader::eager(|cx| async move {
    let rows = cx.master().list_values().await?;
    cx.reconcile(rows).await
});
```

`reconcile` makes the cache hold exactly `rows`: changed rows are written,
vanished ones deleted, and rows with a flash in flight left alone, because
the listing may predate the write. A start that copies page by page through
`fetch_cursor` writes `cx.cache()` instead and resumes from the last cached
key on the next launch; the Dio announces the set once the start returns.

### Paged

`Loader::paged(chunk)` fetches the windows a table view asks for, as it asks
for them. `chunk` receives the context, the window, the view's query and a
`ChunkSink`:

```rust
let loader = Loader::paged(|cx, range, query, sink| async move {
    let fetched = cx.fetch(range.clone(), &query).await?;
    if !fetched.refused.is_empty() {
        return Err(vantage_core::error!(
            "the master refused part of the view's query",
            refused = fetched.refused
        ));
    }
    let page = fetched.page;
    if let Some(total) = page.total {
        sink.set_total(total.max(0) as usize);
    }
    for (i, (id, record)) in page.rows.into_iter().enumerate() {
        sink.push(range.start + i, id, record).await?;
    }
    Ok(())
});
```

- `cx.fetch(window, &query)` applies the query to a clone of the master, so
  differently sorted views never race on the shared one. It returns a
  `Fetched`: the `page`, and every clause the page doesn't reflect in
  `refused`. It never applies those clauses to the window itself (see
  [the residual rule](#the-residual-rule)).
- `sink.push(idx, id, record)` binds one row to a row index of the view. The
  row is visible as soon as `push` resolves; the pushed rows reach the cache
  in one write once the loader returns, and the view repaints once per
  window rather than once per row.
- `sink.set_total(n)` is the only way a paged view learns its total. A
  loader that says nothing leaves the total as it was.

A loader may push fewer rows than it was asked for (the set ran out), out of
order, or across awaits as a slow API trickles in. It may also be dropped
before it finishes, when the viewport moves on. So it must not detach work
it expects to finish. Rows it already pushed are put back the way it found
them: the record that was in the slot, or nothing. A loader that returns
`Err` unwinds the same way, so a failed re-pull leaves the view showing what
it showed before.

`vantage_core::Priority::current()` tells the loader whether a view is
waiting on it (`Essential`) or not (`Background`), for a retry policy that
tries harder when someone is looking.

### Paged with an eager fallback

`Loader::paged(chunk).or_eager(start)` pages a master that advertises
`Window` and copies one that doesn't, so one Lens serves both kinds of
table. The eager half is also what lets a paged view answer a query the
master can't: the view copies the whole set through `start` and evaluates
the leftover clauses over it.

```rust
let loader = Loader::paged(page_window).or_eager(|cx| async move {
    let rows = cx.master().list_values().await?;
    cx.reconcile(rows).await
});
```

`or_eager` on a loader with no paged half fails the Lens build, as does
`blocking` on one with no eager start.

## The residual rule

Every clause the master refuses is evaluated by Diorama over a complete
set, or the read fails loudly. Nothing in between.

What "a complete set" is depends on the read:

- **An eager view** evaluates the clause over the cache, which holds the
  whole set.
- **A paged view** whose master takes the whole query is served window by
  window. If anything is left over, the view copies the whole set through
  the loader's eager start and evaluates it there. With no eager start, the
  view fails: no rows, `load_state()` is `LoadState::Failed(failure)` naming
  the table and the clause, and `LoadFailed` goes out on the bus.
- **`Dio::fetch`** reads one window, so it applies nothing and hands the
  leftovers back in `Fetched::refused`, for the caller to decide.
- **A derived Dio** (`dio.derive().with_condition(..).build()`) keeps what
  its master refused in `dio.residual()`. Every read of it (views, the
  facade, a record scenery, a servo, an aggregate) keeps only the rows the
  residual admits.
- **A query the master rejects** fails every view, eager ones included: an
  unknown column is never evaluated locally.

### Why not filter the window?

Because the result is a lie that looks like an answer. Fifty rows filtered
down to three is not "the three matching rows"; it is three of an unknown
number, presented as the set. The total is wrong, the scrollbar is wrong,
and scrolling shows a different handful each time. A user can work with "this
view can't filter on that column". Nobody can work with a filtered grid that
silently isn't.

## Testing against Diorama

`MockShell` from `vantage_vista::mocks` is the in-memory shell Diorama's own
tests drive, and it doubles as a scriptable source. Its store is shared by
every clone, so a handle taken before the shell goes into a `Vista` changes
what the master reads:

```rust
let source = MockShell::new()
    .with_metadata(metadata)
    .with_record("42", record(&[("title", "before")]));
let control = source.clone();
let dio = lens.dio(Vista::new("items", source)).build().await?;
let view = dio.table_scenery().open().await?;

control.set_field("42", "title", CborValue::Text("after".into()));
dio.refresh().await?;

control.remove_record("42");
control.set_record("99", record(&[("title", "new")]));
control.set_fail_reads(true);
```

`set_fail_reads(true)` makes every read fail, for the outage path. For
latency under virtual time, the BDD suite gates the source's reads behind
`tokio::time::pause()`: see `tests/features/source_control.feature`.

The wider matrix (residuals over real drivers, refusals, paged totals)
lives in `vantage-diorama/tests/`. `tests/residual/drivers.rs` runs a
refused dotted filter over CSV and SQLite and checks that the view fails
rather than showing the unfiltered window.

## Data source checklist

A source ready for Diorama has:

1. **Capabilities** that are all true, checked by
   `contract::check_advertised` against a fixture.
2. **Refusals** (`unsupported(..)`) for what the backend can't do on a call,
   and plain errors for what is wrong, so Diorama evaluates the first and
   fails on the second.
3. **Totals** reported only when the backend knows them.
4. **`Subscribe`** only when the stream pushes with no further setup.
5. **A loader** that matches the master: eager for sets that fit, paged for
   `Window` masters, `or_eager` for a Lens that serves both or for views
   whose queries the master can't take whole.
