# Augmentation — Enriching Rows

Chapter 5's inventory knows every station file's name and size — the two things an S3 listing
gives away for free. Everything interesting is *inside* the files: how many readings a station
has recorded, and how recently. Getting that costs one download per file, which is exactly the
kind of expense you want to pay once and remember.

**Augmentation** is Diorama's answer: enrich the master's rows, one row at a time, from a
*detail* source — and let the Dio's cache hold the result. The listing stays the cheap, fast
spine it was in chapter 5; each row *gains columns* it never had, hydrated on demand and
persisted alongside the row. This chapter adds two such columns to the inventory:

```text
      SIZE     ROWS     LATEST  FILENAME
   4796352   140629   19911231  csv/by_station/GM000001153.csv
```

`ROWS` — how many readings the file holds; `LATEST` — the date of the most recent one. Neither
exists anywhere in S3's listing; both are computed from the file's contents.

We build on chapter 5's crate unchanged — `learn-5` starts as a copy of `learn-4`
(`files.rs` and `main.rs` exactly as they were) and this chapter only adds. One new file,
`readings.rs`, describes the detail side; a handful of lines in `main.rs` wire it up.

---

## The detail side: `readings.rs`

Every station file is a CSV of date-ordered readings:

```text
ID,DATE,ELEMENT,DATA_VALUE,M_FLAG,Q_FLAG,S_FLAG,OBS_TIME
GM000001153,18910101,TMAX,-20,,,I,
GM000001153,18910102,TMAX,-40,,,I,
...
```

So the two columns we're after are cheap *derivations*: `rows` is the line count minus the
header, `latest` is the `DATE` field of the last line. The expensive part is getting the
contents at all.

The detail source is a table — the same `ListObjectsV2` listing as the master, in fact, with two
differences: it declares **no prefix** (the augmentation will narrow it to a single file per
fetch), and it carries the derived columns. Start `readings.rs` with its entity:

```rust
use serde::{Deserialize, Serialize};
use vantage_aws::prelude::*;

/// One file, seen through the augmenter: the listing row plus columns
/// computed from the file's contents.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Readings {
    #[serde(rename = "Key")]
    pub filename: String,
    pub rows: i64,
    pub latest: String,
}
```

### Lazy expressions

Where do `rows` and `latest` come from? Chapter 2's `with_expression` won't do it — those
expressions lower *into the backend's query* (a SQL subselect), and S3 can't compute anything
server-side. What we need runs on our side, **after** the row comes back.

That's a **lazy expression**: a column computed in Rust, on the returned record. Lazy
expressions apply in declaration order, and each callback *borrows the record as built so far* —
including the columns earlier lazy expressions added. That ordering rule is the whole trick:

```rust
impl Readings {
    pub fn table(aws: AwsAccount, bucket: &str) -> Table<AwsAccount, Readings> {
        let bucket = bucket.to_string();
        Table::new("restxml/Contents:s3/GET /{Bucket}?list-type=2", aws.clone())
            .with_id_column("Key")
            .with_condition(eq("Bucket", bucket.clone()))
            .with_lazy_expression("contents", move |row| {
                let aws = aws.clone();
                let bucket = bucket.clone();
                let key = row.get("Key").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                async move { Ok(s3::get_object(&aws, &bucket, &key).await?.into()) }
            })
            .with_lazy_expression("rows", |row| {
                // Every line after the CSV header is one reading.
                let contents = row.get("contents").and_then(|v| v.as_str()).unwrap_or_default();
                let rows = contents.lines().count().saturating_sub(1) as i64;
                async move { Ok(rows.into()) }
            })
            .with_lazy_expression("latest", |row| {
                // Readings are date-ordered; take the last line's DATE column.
                let contents = row.get("contents").and_then(|v| v.as_str()).unwrap_or_default();
                let latest = contents
                    .lines()
                    .last()
                    .and_then(|line| line.split(',').nth(1))
                    .unwrap_or_default()
                    .to_string();
                async move { Ok(latest.into()) }
            })
    }
}
```

Read it as a pipeline over one record. The listing returns `{Key, Size, …}`; then:

1. **`contents`** reads `Key` off the record and downloads the file —
   `s3::get_object` is the driver's raw fetch, unsigned here just like the listing. Its return
   value is inserted into the record under `contents`. This is the expensive step, and it
   happens **once**.
2. **`rows`** never touches the network: it reads `row.get("contents")` — the column the
   previous expression just added — and counts lines.
3. **`latest`** reads the same `contents` and takes the last line's date.

One download feeds every derived column declared after it. Each callback clones what it needs
out of the borrowed record before going async, and each expression's name is also registered as
a column on the table, so the derived fields are part of its schema like any other.

```admonish info title="Lazy expressions from YAML"
Like most things declared on a table, lazy columns have a config-driven form: a column spec may
carry `lazy: <rhai script>`, where the script sees the record built so far as `row` and its
final expression becomes the value — `row.contents.split("\n").len() - 1`. Declaration order
chains the same way. See [Config-Driven Vistas](../config-driven-vistas.md).
```

## Why not just list this table?

It's tempting to stop here — the augmenter table already produces every column we want, so why
not make *it* the Dio's master? Because lazy expressions run **when data is fetched**. A
`list()` on this table triggers the whole pipeline for every row it returns: a thousand files
means a thousand downloads inside one blocking call, and nothing comes back until the last one
lands. That's not a listing — it's a batch job wearing a listing's interface.

What we're after is different. The list of files should reach the user **instantly** — it's
chapter 5's cached listing, it costs milliseconds. The expensive columns should then be fetched
**one row at a time**, each result written into the Dio and announced on the event bus, so
anything watching updates progressively. Loading details for every station takes a long time no
matter what; the win is in never blocking the listing on it, and in choosing the *order* — the
next chapter puts a viewport in charge, so the rows the user is currently looking at are
hydrated first, and rows nobody scrolls to are never fetched at all.

That split — a cheap master everyone lists, an expensive detail source consulted per row — is
what **augmentation** declares.

## Wiring the augmentation

Back in `main.rs`. The augmenter becomes a Vista like any table, and the Dio is told how to use
it — this is the only structural addition to chapter 5's `run()`:

```rust
let augmenter = aws
    .vista_factory()
    .from_table(Readings::table(aws.clone(), BUCKET))?;

let dio = lens.make_dio(master).await?.augment(
    Arc::new(VistaCatalog::new()),
    vec![Augmentation {
        detail: Detail::Fixed(Arc::new(augmenter)),
        source: Source::Column {
            from: "Key".into(),
            to: Some("prefix".into()),
        },
        fetch: Fetch::PerRow,
        merge: MergeRule {
            columns: vec!["rows".into(), "latest".into()],
        },
    }],
);
```

An [`Augmentation`](vantage_diorama::Augmentation) answers four questions:

- **`detail`** — *where do detail records come from?* `Detail::Fixed` holds our augmenter Vista
  directly. (`Detail::Catalog(name)` resolves one by name instead — the config-driven form,
  which is also why `augment` takes a [`VistaCatalog`](vantage_vista_factory::VistaCatalog).
  Building an empty catalog just to satisfy the signature is admittedly awkward; the parameter
  earns its place only in the catalog form.)
- **`source`** — *how does a master row select its detail record?* `Source::Column` maps the
  master's `Key` onto the detail table's `prefix` condition. A full filename used as an S3
  prefix matches exactly one object — so each fetch narrows the augmenter to a one-row listing,
  and the lazy expressions run for precisely that file.
- **`fetch`** — `Fetch::PerRow`: one detail fetch per master row.
- **`merge`** — *which detail columns land on the master row?* Just `rows` and `latest`.
  `contents` is deliberately absent: it exists only inside the detail fetch, feeds the derived
  columns, and is never cached. The megabytes stay out of the Dio; the two numbers stay in.

## Reads hydrate

Nothing has fetched anything yet — declaring an augmentation is free. The work happens on
*read*: rows a facade read returns come back hydrated — any of them still missing its augment
columns runs the detail fetch first, and the result is written back to the cache as complete.

<svg viewBox="0 0 760 330" xmlns="http://www.w3.org/2000/svg" font-family="sans-serif" font-size="13">
  <defs>
    <marker id="arrow3" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="#888"/>
    </marker>
  </defs>

  <!-- Dio container -->
  <rect x="20" y="30" width="300" height="170" rx="12" fill="none" stroke="#8f5a2d" stroke-width="2.5"/>
  <text x="170" y="54" text-anchor="middle" fill="#8f5a2d" font-weight="bold" font-size="15">Dio</text>

  <!-- master -->
  <rect x="45" y="75" width="110" height="80" rx="8" fill="#4a7c59"/>
  <text x="100" y="107" text-anchor="middle" fill="#fff" font-weight="bold">master</text>
  <text x="100" y="127" text-anchor="middle" fill="#fff" font-size="12">S3 listing</text>

  <!-- cache -->
  <rect x="185" y="75" width="110" height="80" rx="8" fill="#4a7c59"/>
  <text x="240" y="107" text-anchor="middle" fill="#fff" font-weight="bold">cache</text>
  <text x="240" y="127" text-anchor="middle" fill="#fff" font-size="12">redb</text>

  <!-- facade Vista -->
  <rect x="560" y="55" width="170" height="70" rx="10" fill="#7c2d8f"/>
  <text x="645" y="84" text-anchor="middle" fill="#fff" font-weight="bold">facade Vista</text>
  <text x="645" y="104" text-anchor="middle" fill="#fff" font-size="12">dio.vista()</text>

  <!-- request: facade → dio -->
  <line x1="556" y1="72" x2="324" y2="72" stroke="#888" stroke-width="2" marker-end="url(#arrow3)"/>
  <text x="440" y="62" text-anchor="middle" fill="#888" font-size="12">get / window — you ask</text>

  <!-- response: dio → facade -->
  <line x1="324" y1="108" x2="556" y2="108" stroke="#888" stroke-width="2" marker-end="url(#arrow3)"/>
  <text x="440" y="126" text-anchor="middle" fill="#888" font-size="12">hydrated rows — gaps filled first</text>

  <!-- per-row down: dio → augmenter -->
  <line x1="90" y1="200" x2="90" y2="256" stroke="#888" stroke-width="2" marker-end="url(#arrow3)"/>
  <text x="105" y="222" fill="#888" font-size="12">per row with a gap:</text>
  <text x="105" y="240" fill="#888" font-size="12">Key → prefix</text>

  <!-- per-row up: augmenter → dio (cache) -->
  <line x1="250" y1="256" x2="250" y2="204" stroke="#888" stroke-width="2" marker-end="url(#arrow3)"/>
  <text x="265" y="222" fill="#888" font-size="12">{rows, latest} merged into the cache,</text>
  <text x="265" y="240" fill="#888" font-size="12">RecordChanged on the bus</text>

  <!-- augmenter Vista -->
  <rect x="20" y="260" width="300" height="60" rx="8" fill="#4a7c59"/>
  <text x="170" y="284" text-anchor="middle" fill="#fff" font-weight="bold">augmenter Vista — Readings</text>
  <text x="170" y="304" text-anchor="middle" fill="#fff" font-size="12">lazy: contents → rows → latest — one download</text>
</svg>

Not every read, though. The listing must stay what chapter 5 made it — instant — so
`list_values` through the facade returns the cheap rows untouched. Hydration belongs to
**bounded reads**: `get_value` for one record, `fetch_window` for a range. The rows you ask for
are the rows that pay:

```rust
// The listing stays instant — cheap rows, no downloads.
let listing = dio.vista().list_values().await?;
println!("{} files (listed in {:?})", listing.len(), start.elapsed());

// Details are paid for by the rows you ask for: a window of ten.
let window = dio.vista().fetch_window(0, 10).await?;
for (filename, file) in &window {
    let size = file.get("Size").and_then(|v| v.as_str()).unwrap_or("");
    let rows = file.get("rows").and_then(|v| v.as_i64()).unwrap_or(0);
    let latest = file.get("latest").and_then(|v| v.as_str()).unwrap_or("");
    println!("{size:>10} {rows:>8} {latest:>10}  {filename}");
}
```

Ten downloads is still a wait worth narrating, and the Dio announces it: one
`DioEvent::Hydrating` with the pending count before the first fetch, then a `RecordChanged` per
row as each lands. A dozen lines of event plumbing become the progress display:

```rust
let mut events = dio.subscribe_events();
tokio::spawn(async move {
    while let Ok(event) = events.recv().await {
        match event {
            DioEvent::Hydrating { pending } => println!("hydrating {pending} files…"),
            DioEvent::RecordChanged { id } => println!("  {id}"),
            _ => {}
        }
    }
});
```

First run — chapter 5's sync, the instant listing, then the window doing its downloads:

```text
fetched 100 files in 215ms
...
1122 files (listed in 16ms)
hydrating 10 files…
  csv/by_station/GM000001153.csv
  csv/by_station/GM000001474.csv
  ...
   4796352   140629   19911231  csv/by_station/GM000001153.csv
   7057976   206874   20260531  csv/by_station/GM000001474.csv
   2978238    87207   19990131  csv/by_station/GM000002277.csv
   ...
10 files detailed in 20.2s
```

Real data, and readable at a glance: station `GM000001474` has 206,874 readings and is still
reporting (May 2026); its neighbour `GM000001153` went silent at the end of 1991.

Second run — new process, warm cache:

```text
fetched 0 files in 340ms
1122 files (listed in 16ms)
   4796352   140629   19911231  csv/by_station/GM000001153.csv
   ...
10 files detailed in 16ms
```

No `hydrating` line at all: those ten rows already carry their augment columns, so the window
finds no gaps and never touches the network. Twenty seconds became sixteen milliseconds; the
derived numbers live in `cache.redb` with the rest of the row. Ask for a different window —
`fetch_window(500, 10)` — and only *its* gaps download. (`--invalidate` still clears everything,
derived columns included — derived data is data.)

```admonish question title="What if a download fails?"
Nothing is swallowed. Each failing row broadcasts `DioEvent::RecordLoadFailed { id, error }`
on the bus — the same listener printing our progress can print failures beside it. The bounded
read that requested the row then returns an error ("augment hydration failed") rather than
handing you a window that quietly misses data. Retry by asking again: rows that *did* land are
already cached and are never re-downloaded. Chapter 7's reactive views are more forgiving — a
failed row is marked `RowStatus::LoadFailed` while its cheap listing columns stay visible, and
the fetch is retried the next time the viewport reaches for it.
```

---

## What we covered

| Concept                                             | What it does                                                     |
| --------------------------------------------------- | ----------------------------------------------------------------- |
| `with_lazy_expression(name, callback)`              | A column computed in Rust on the returned record; chains in order |
| `s3::get_object`                                    | Raw object fetch through the same (unsigned) driver               |
| [`Augmentation`](vantage_diorama::Augmentation)     | Declares detail source, row→detail mapping, fetch style, merge    |
| `Detail::Fixed` / `Detail::Catalog`                 | Detail Vista held directly, or resolved by name from a catalog    |
| `Source::Column { from, to }`                       | Master column → detail condition (our `Key` → `prefix`)           |
| `MergeRule`                                         | Which detail columns land on the master row — and which don't     |
| Bounded facade reads hydrate                        | `get_value` / `fetch_window` fill gaps for the rows they return; `list_values` stays cheap |
| `DioEvent::Hydrating`                               | Fired before a hydration sweep — the cue for progress UI          |

```admonish tip title="What's next"
Our CLI asks for one fixed window and exits. A real interface is a *standing* view: the window
should follow the user's cursor, rows should repaint as details land, and the whole thing should
stay current as the bucket changes. The next chapter opens **Sceneries** over this same Dio — a
live terminal UI scrolling the full 122,000-station archive, hydrating the rows the user is
looking at.
```

```admonish info title="Going deeper"
Augmentation has a reference chapter of its own — batched fetches, the gap rule, demand gating,
and the YAML form — at [Augmentation](../augmentation.md).
```
