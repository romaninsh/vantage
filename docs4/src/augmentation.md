# Augmentation

A `Vista` reads one table from one backend. But a row is often only half the
story: an S3 bucket listing tells you a file's key and size, not how many
resources the terraform state inside it declares. A REST list of repositories
doesn't carry the CI status that lives behind a shell command. The data you want
to show lives in a *second* source, keyed off the first.

**Augmentation wires two Vistas into one `Dio`**: a *master* that is listed, and
a *detail* source that is loaded one row at a time and merged on top. The two can
be the same Vista (a backend whose list and detail are separate operations) or
entirely different backends — a REST master enriched by a `cmd` detail, or the
reverse. The detail source is resolved by name through the
[`VistaCatalog`](./new-persistence/step8-vista-integration.md), so it is
persistence-agnostic.

## Why this lives in the Dio, not the Vista

This is a join, and joins are governed by the capability contract. A SQL Vista
can join its own tables; a REST Vista cannot; two *different* backends can never
push a join down to either engine. Where the backend can't, the layer above fills
the gap — that is the whole job of the Dio.

So augmentation is deliberately a `Dio`-layer feature. A `Vista` honestly
describes one backend and has no cache, no viewport, and no catalog; declaring
"enrich from another Vista, loaded lazily and cached" on it would be a promise it
can't keep. The `Dio` has all three, so it owns the augmentation: it lists the
master cheaply, and for each row the user actually scrolls to, it fetches the
detail and stitches the columns together. It is a per-row join, executed
client-side, exactly where a push-down is impossible.

```admonish note title="It reduces to the old two-pass"
The previous progressive-loading path — a `cmd` table with a `detail` script —
is just the special case where the detail source *is* the master and the key is
the id. Expressed as augmentation it is `table: <self>, source: id, fetch:
per_row`. Nothing about that behaviour changed; it stopped being a backend quirk.
```

## How it runs

Augmentation engages the two-pass machinery:

1. **List pass** — the master is listed cheaply (a page at a time). Each row is
   written to the cache as `Incomplete`, carrying only the columns the list
   returns. When the master can serve windows (`can_fetch_window`) the page is
   pushed down; otherwise the set is listed and windowed locally.
2. **Detail pass** — viewport-driven. For each visible `Incomplete` row, every
   augmentation resolves its detail Vista, fetches the matching record, and
   merges the chosen columns onto the cached row, which flips to `Fresh`. Rows
   off-screen are never fetched; rows already hydrated are never re-fetched.

A failed detail fetch marks only that row, leaving its cheap columns intact — the
rest of the page hydrates normally.

## Declaring it

On the `Lens`, supply the catalog and one or more augmentations:

```rust
let lens = Lens::new()
    .cache_at(cache_path)
    .catalog(catalog)                       // resolves `table:` names
    .augment(vec![augmentation])            // ≥1 engages two-pass
    .build()?;
```

Each `Augmentation` has four parts:

```rust
Augmentation {
    table:  "tfstate_detail".into(),  // catalog name of the detail Vista
    source: Source::Column { from: "key".into(), to: None },
    fetch:  Fetch::PerRow,
    merge:  MergeRule { columns: vec!["resources".into(), "serial".into()] },
}
```

`source` and `fetch` are two orthogonal axes:

| `Source` | how a master row selects its detail |
|---|---|
| `Id` | `master.id → detail.id` |
| `Column { from, to }` | `master[from] → detail[to` *or* `detail.id]` |
| `Build(closure)` | arbitrary narrowing from the whole row — per-row only |

| `Fetch` | how the detail is read |
|---|---|
| `PerRow` | one record per master row (`get_value`, or narrow-and-take-first) |
| `Custom(closure)` | a caller-supplied async fetch |
| `Batched` | one set query across the window's keys — *planned* |

`Id` and id-keyed `Column` sources read by key through `get_value` — the uniform
"one record by key" primitive (a `cmd` detail script, a SQL `WHERE id =`, a REST
`GET /{id}`). They can name a target column, so they are the batchable shapes.
`Build` returns an arbitrary narrowed Vista that can't be coalesced into a set, so
it is per-row only — the type enforces what can and can't batch, rather than a
runtime check.

### From YAML

The same declaration is data, lowered with `lower_augment`:

```yaml
augment:
  - table: tfstate_detail
    source: { kind: column, from: key }
    fetch:  { kind: per_row }
    merge:  [resources, serial, outputs]
```

### Rhai as a closure factory

The runtime types carry **closures**, not script strings — `Source::Build` is a
`Fn(&row, base) -> Vista`. Rhai is one factory that produces such a closure;
hand-written Rust is another. A `{ kind: script, code: "..." }` source lowers (under
the `rhai` feature) to a `Build` closure that narrows the base detail Vista using
the master `row`:

```rhai
self.add_condition_eq("key", row.key)
```

This is the same machinery a reference build-script uses, pointed at a possibly
different persistence. The diorama core never sees a script string; all engine
assembly stays in `vantage-vista`.

## Merging

`merge.columns` lists the detail columns to lift; an empty list lifts all of
them. The detail record is the authoritative hydration of the row, so on a name
clash its value wins — it overwrites the cheap list-pass value and adds its new
columns. (An empty list plus overwrite is exactly the old cmd two-pass: the
detail script returns the full record, which replaces the stub.) The augmented
`Dio` therefore advertises the union of the master's columns and the lifted
detail columns — the "Dio advertises a superset" principle: below it, sources are
partial; above it, the view is whole.

## Anticipated objections

**Why not model the detail as a foreign-key relation?** A relation resolves
*within* one persistence and can be pushed down as a join. The detail here may be
a different backend entirely; there is nothing to join on and no engine that could
honour it. Augmentation is the cross-Vista form, stitched by the Dio.

**Why one row at a time?** Because the expensive work should follow the user's
attention. The list pass is cheap and immediate; only rows that reach the viewport
pay for their detail, and they pay once.

## What's next

Three things are planned but not yet implemented; each fails honestly today rather
than degrading silently:

- **`Batched` fetch** — collect the window's distinct keys into one set query and
  scatter the results back, for `Id`/`Column` sources.
- **Detail-key cache** — dedupe fetches across master rows that share a key
  (non-unique `Column` sources).
- **Scripted fetch** — Rhai fetch verbs choosing how to pull.

## Checklist

To augment a master with a second source:

1. **Register the detail model** in the `VistaCatalog` by name.
2. **Build the `Augmentation`** — pick a `Source` (id, column, or a closure) and a
   `Fetch` (`PerRow` for now), and list the `merge` columns.
3. **Wire the Lens** — `.catalog(...)` then `.augment(vec![...])`; building it
   engages the two-pass passes automatically.
4. **Show the columns** — the lifted detail columns appear on hydrated rows
   alongside the master's.

A runnable end-to-end example (two in-memory Vistas, list pass then detail pass)
lives in `vantage-diorama/examples/augmentation.rs`:

```console
cargo run -p vantage-diorama --example augmentation
```
