# Fetching What You Drop

This section examines the requests that the application did not have to send. These are more
difficult to find. A wasteful application and an efficient application look the same on screen.

There are three types of waste, from the easiest to find to the most difficult:

1. **Repeats.** The application fetched the same window more than one time.
2. **Redundant rows.** Rows arrived when the cache already held them.
3. **Columns that no view shows.** Data crossed the network and then went in the bin.

The first two have counters. The third has one line.

## The two counters that must be zero

Quit Vantage UI. The session summary prints:

```text
people     summary  3 fetches, 0 repeats · 400 rows, 0 redundant · 9.1s waiting, 3.0s slowest
           summary  1 dio, 1 table scenery, 0 record, 0 servo still alive
           summary  session 18.0s · cpu 15.0s · peak rss 352MB
```

**`repeats`** counts the windows that the application requested more than one time. The loader is
built to avoid repeats, so a number above zero is a defect. There are three causes:

- the cache is not operating
- a total declares rows that the source will not serve
- a viewport moves between two positions

**`redundant`** counts the rows that arrived when the cache already held them. A cold run must give
zero. A warm run gives a small number, and that is the cost of revalidation. If the redundant count
equals the received count, the application fetched a full page for nothing.

```admonish note title="Read the counters with the session, not alone"
`3 fetches` has no meaning until you know that you scrolled through 400 rows of a table of 200,000.

Use the ratios: fetches for each screen of rows, and redundant rows for each received row. Record
what you did together with what it cost.
```

## Watch the cache do its work

```text
22:42:19.028  people   viewport  rows 77..90 — 6 scroll events coalesced into one
22:42:19.029  people   cache     all 13 rows of 77..90 served locally — no fetch
22:42:19.516  people   viewport  rows 125..138 — 3 scroll events coalesced into one
22:42:19.516  people   cache     all 13 rows of 125..138 served locally — no fetch
22:42:19.858  people   viewport  rows 257..270 — 11 scroll events coalesced into one
22:42:19.859  people   dio       fetch #2 asks for rows 257..357 (viewport 257..270) — 100 missing
```

Two efficiencies are visible here.

**The framework combines scroll events.** Twenty scroll events produced three viewports. The loader
waits for the movement to stop, and then declares one viewport for the final position. The
intermediate positions never cause a fetch.

**The loader reads ahead of the screen.** The viewport of `fetch #2` was `257..270`, which is 14
rows. The request was for `257..357`, which is 100 rows. The loader fetches extra rows in the
direction of movement, so that the next screen costs nothing. This is why the two ranges differ.

Read-ahead also explains the fetch count in the summary above: 3 fetches covered 400 rows, which is
more screens than three.

## The waste with no counter

Now add a source with large rows. The grid shows five short columns. Each row carries 200 more
fields that nobody declared.

```yaml
# datasource/wide.yaml
type: faker
effect: static
debug: true
count: 500
seed: 111
shape:
  capabilities: [count, fetch_window, order]
  latency:
    window: 50ms..150ms
  extra_fields:
    count: 200       # 200 extra fields in each row
    size: 500        # each extra field holds 500 bytes
```

Open it and read one line:

```text
22:06:34.148  wide  dio      fetch #1 got 200 rows in 2.3s  ⚠ slow
22:06:34.148  wide  payload  206 columns received · no view declared what it needs, so all were
                             fetched · 19.6MB · id,name,surname,email,city,balance,extra_0001,
                             extra_0002 +198 more
```

**The application moved 19.6MB to fill a grid of five columns.** The five columns are about 19KB of
that total.

Nothing failed. No counter changed. `repeats` is zero, and `redundant` is zero. Each measurement in
the session summary says that the application is correct.

This fault hides better than the others, because each other signal says that the application is
correct. Only the `payload` line finds it.

## "No view declared what it needs"

Diorama has a mechanism for this. A scenery can declare its **demand**, which is the list of columns
that it shows. The Dio then combines the demands of the open views. When a view declares its demand,
the line reports against that demand:

```text
wide  payload  5 of 206 columns wanted · 201 fetched and dropped · 19.6MB
```

This grid declares nothing. The combined demand is therefore "all columns".

The framework cannot tell the difference between a view that wants all 206 columns and a view that
never declared its columns. It assumes the first, and it fetches all the columns.

```admonish note title="Demand controls the second pass, not the width of the first"
Diorama can load a row in two passes. The first pass fetches the window. The second pass — the
**hydrate** pass — fetches the augmented columns of the rows on screen.

Demand controls the second pass. A view that does not show an augmented column does not pay to
hydrate it.

Demand does not yet decrease the columns in one window fetch. The 19.6MB above is therefore not a
configuration error that you can correct in YAML. Use the `payload` line as a measurement, and use
the size of the number as the reason to make the source return fewer columns.
```

## What to do about a large payload

These actions are listed from the most effective to the least effective.

- **Decrease the columns at the source.** Use a REST endpoint with a `fields=` parameter, a SQL view
  that selects the columns that you show, or a GraphQL query that names its columns. The cheapest
  byte is the byte that nobody sends.
- **Move the large columns into an augment.** An `augment:` block on the table makes the large
  fields the second pass, which demand controls. See [Augmentation](../augmentation.md) for the
  block and its options.
- **Observe fewer tables.** A page that observes two tables opens two Dios and pays for both.

## A checklist for one page

Run the page cold, then warm. Read four numbers.

| Number | Where | Correct value |
| --- | --- | --- |
| fetches | `summary` | fewer than one for each screen, because the loader reads ahead |
| repeats | `summary` | **zero**, always |
| redundant | `summary` | zero when cold; small when warm |
| payload for each fetch | `payload` | proportional to the columns that you show |

If the first three numbers are correct and the fourth is not, you have the invisible fault.

---

## What we covered

| Item | What it does |
| --- | --- |
| `repeats` | Windows that the application fetched more than one time. Must be zero. |
| `redundant` | Rows that arrived when the cache held them. Zero when cold, small when warm. |
| coalescing | Scroll events that became one viewport, because the loader waits for the movement to stop |
| read-ahead | The extra rows that the loader fetches in the direction of movement |
| `payload` | The columns that arrived, the columns that a view wants, and the cost in bytes |
| `extra_fields` | The faker setting that adds fields to each row |
| demand · hydrate | The columns that a view declares, and the second pass that they control |

```admonish tip title="What is next"
Each source until now answered its requests. The next section uses a source that fails one request
in five, stops for ten seconds each minute, and reports a row count that is wrong.
```
