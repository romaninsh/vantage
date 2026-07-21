# Relations and Dio

Everything in this guide so far has executed at or below the Vista: typed relations on the Table,
traversal narrowing, subquery expressions, implicit references. This final chapter climbs one layer
higher. A `Dio` keeps a live local copy of a data segment (a cache), reads through a `Lens`,
announces changes on an event bus, and serves reactive views (`Scenery`). You built one in the intro
paths ([Dio over SQL](../intro/step5-sql-dio.md), [Scenery](../intro/step7-scenery.md)).

Two questions matter here: what happens to the relation machinery you already know once a Dio sits
on top — and what relation shape exists *only* at the Dio layer, because nothing below it can
express one.

### Same-persistence relations resolve beneath the Dio

Traversal, subquery expressions, and implicit references all happen in the Table or Vista *under*
the Dio. The Dio caches the resulting rows like any others. If your table declares
`with_active_columns(&["id", "client.name"])`, the `client.name` column is already part of each row
when it enters the cache — nothing at the Dio layer knows or cares that it was traversed. From the
cache's point of view, a traversed column and a plain column are indistinguishable.

The practical consequence is the master/detail pattern in a UI: **traverse at the Vista, cache at
the Dio**. Take a row from the cached master, call `Vista::get_ref(relation, row)` to build the
narrowed detail Vista — the same erased traversal from the [Vistas chapter](./vistas.md), or
`VistaCatalog::traverse_from` when the detail may live in another datasource — and put a
Dio in front of *that* for the detail pane. Each narrowed detail set gets its own cache entry, so
switching between master rows switches between already-cached detail sets rather than re-narrowing
one shared cache.

### The join the layers below cannot express

The [`VistaCatalog`](./vistas.md) can already *navigate* across datasources — hand it a parent row
and it returns the related set from another backend. What nothing below the Dio can do is *merge*:
show master rows whose columns come from two sources at once. That is a join, and joins are
governed by the capability contract — a SQL Vista can join its own tables, a REST Vista cannot,
and two different backends can never push a join down to either engine. A Vista also has nowhere
to put stitched rows: no cache, no viewport, no notion of "only visible rows pay".

Where the backend can't, the layer above fills the gap — that is the whole job of the Dio.
**Augmentation wires two Vistas into one `Dio`**: a *master* that is listed, and a *detail* source
loaded one row at a time and merged on top. The detail is resolved by name through the
`VistaCatalog`, so it is persistence-agnostic — a REST master enriched by a `cmd` detail, or a SQL
master enriched from another database.

It runs in two passes:

1. **List pass** — the master is listed cheaply; each row enters the cache marked `Incomplete`.
2. **Detail pass** — viewport-driven: for each *visible* incomplete row, the augmentation resolves
   its detail Vista, fetches the matching record, merges the chosen columns, and the row flips to
   `Fresh`.

Rows off-screen are never fetched; hydrated rows are never re-fetched. A failed detail fetch marks
only that row.

### Declaring an augmentation

Augmentations are configured on the `Lens`. The catalog is what makes the detail side resolvable by
name; supplying at least one augmentation is what engages the two-pass behavior:

```rust
let lens = Lens::new()
    .cache_at(cache_path)
    .catalog(catalog)                       // resolves `table:` names
    .augment(vec![augmentation])            // ≥1 engages two-pass
    .build()?;
```

Each augmentation answers four questions — which detail Vista, how a master row selects its detail,
how it is fetched, and which columns to lift:

```rust
Augmentation {
    table:  "tfstate_detail".into(),  // catalog name of the detail Vista
    source: Source::Column { from: "key".into(), to: None },
    fetch:  Fetch::PerRow,
    merge:  MergeRule { columns: vec!["resources".into(), "serial".into()] },
}
```

- `source` picks how a master row selects its detail: `Source::Id` matches master.id → detail.id;
  `Source::Column { from, to }` matches master[from] → detail[to], or detail.id when `to` is `None`;
  `Source::Build(closure)` performs arbitrary narrowing from the whole row, and is per-row only.
- `fetch` picks how the detail is read: `Fetch::PerRow` today; `Batched` is planned.
- `merge.columns` lists the detail columns to lift. Empty means all; on a name clash the detail
  wins.

The same declaration works from YAML:

```yaml
augment:
  - table: tfstate_detail
    source: { kind: column, from: key }
    fetch:  { kind: per_row }
    merge:  [resources, serial, outputs]
```

```admonish info title="Script sources"
A `{ kind: script, code: "self.add_condition_eq(\"key\", row.key)" }` source lowers (under the
`rhai` feature) to a `Build` closure — the same machinery as a reference build-script, pointed at a
possibly different persistence.
```

To see the two passes run, try the example — two in-memory Vistas, list pass then detail pass:

```
cargo run -p vantage-diorama --example augmentation
```

The full treatment of augmentation — merge rules, capability reasoning, the planned `Batched` fetch
and keyed caching — is in the [Augmentation chapter](../augmentation.md); the tutorial form is
[intro step 6](../intro/step6-augmentation.md).

### Why not model the detail as a foreign-key relation?

A relation resolves *within* one persistence and can be pushed down as a join. The detail here may
be a different backend entirely; there is nothing to join on and no engine that could honour it.
Augmentation is the cross-Vista form, stitched by the Dio.

### Choosing between implicit references and augmentation

Both produce the same user-visible result: a row enriched with related fields. Which one to use
depends on where the data lives and who should pay for the fetch.

#### Path A: implicit references (same datasource, in-query)

One query; the backend does the work. The value is present on every row, including off-screen ones,
it is validated when the table is constructed, and it carries read-only column semantics. Choose
this when both
tables live in one datasource that supports traversal (SQL, SurrealDB) — see
[Implicit References](./implicit-references.md).

#### Path B: augmentation (any two datasources, per-row client-side join)

Two passes; expensive work follows the user's attention — only viewport rows pay, and only once.
It works across arbitrary backends, and failed details degrade per-row, not per-page. Choose this
when the sources differ, or when the detail is expensive and you want viewport-driven hydration
even within one backend.

The key difference: implicit references push the join *down* into the engine; augmentation lifts it
*up* into the cache. Same user-visible result — a row enriched with related fields — chosen by where
the data lives and who should pay for the fetch.

### Guide conclusion

This closes the Relations & Traversal guide. At this point you should be able to:

1. **Say where each relation form executes** — traversal and implicit references in the engine,
   erased traversal through the Vista handle, augmentation in the Dio's cache.
2. **Build master/detail** — `Vista::get_ref(relation, row)` for the narrowed detail Vista, with a
   Dio (and its own cache entry) per detail set.
3. **Declare an augmentation** — catalog + source + fetch + merge — in Rust or YAML.
4. **Choose deliberately** between implicit references and augmentation based on where the data
   lives and who should pay for the fetch.

Relations in Vantage are declared once on the model and then travel — into queries, through
erasure, and up into the cache — each layer serving the form the one below can't.
