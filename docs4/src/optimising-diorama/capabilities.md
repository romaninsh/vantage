# Where the Work Happens: Capabilities & Latency

Until now the source was capable. When the source can do everything, the layers above it do nothing
that you can measure.

This section removes the capabilities. **Work does not disappear when a source cannot do it. The
work moves.**

## One line controls everything

Each session starts with this line:

```text
people   source   can count, window, order, search · pages lazily, 100 rows at a time
```

That line is the full contract. The `capabilities:` list in the datasource produces it, and each
later decision follows from it. Change that line, and you change the behaviour of the application.
You change no other file.

## A source that can do nothing

Remove all capabilities. One operation remains, because a source always supports `list`.

```yaml
# datasource/stubborn.yaml
type: faker
effect: static
debug: true
count: 3000
seed: 109
shape:
  capabilities: []          # no count, no order, no search, no windows
  latency:
    list: 2500ms..3500ms    # and the one operation that it has is slow
```

Open the page. The stream is very short:

```text
22:02:51.204  stubborn   dio      created over "stubborn" — cache table "stubborn_stubborn", nothing held yet
22:02:51.205  stubborn   source   can do nothing but list · cannot count, order, search · copied whole into the cache
22:02:51.205  stubborn   scenery  opened cold — 0 rows seeded from cache
22:02:51.206  stubborn   census   +1 table scenery — now 1 table, 0 record, 0 servo · rss 129MB
22:02:54.265  stubborn   scenery  opening → complete (reactor reseed)
22:02:54.337  stubborn   cache    all 22 rows of 0..22 served locally — no fetch
```

**There are no fetch lines.** The `people` source sends one request for each screen of rows. This
source receives one read only — the three seconds between the census line and the
`opening → complete` line. Each later scroll comes from the cache, because the cache holds all the
rows.

Diorama has two load methods, and the `source` line names the one in use.

| Method | Log string | When the framework uses it |
| --- | --- | --- |
| **paged** | `pages lazily` | The source can serve `[offset, limit)` windows |
| **eager** | `copied whole into the cache` | The source cannot serve windows |

You do not select the method. It follows from what the source can do.

| | **paged** | **eager** |
| --- | --- | --- |
| Reads | one window for each screen, on demand | one full read at open |
| Memory | proportional to the rows that you looked at | proportional to the whole table |
| Scrolling | can cost a request | costs nothing |
| A large table | stays cheap | is a large read |

Note the line `opening → complete`, and not `→ partial`. A paged view stays *partial* for most of
its life: it holds 200 of 200,000 rows, and it knows this. An eager view goes directly to complete,
because after the copy there is nothing more to get.

```admonish note title="Eager is correct for a small table, not for a large one"
This source holds 3,000 rows. The same shape with 200,000 rows would read all of them at open, and
the user would wait.

If your source cannot serve windows and your table is large, do not look for a Diorama setting. Add
windows to the source, or observe fewer rows.
```

## The cost of a sort, and its location

Sort the windowed table. The stream tells you where the work goes:

```text
people   sort     by name descending — pushed to the source
```

The next fetch then includes the sort:

```text
people   dio      fetch #3 asks for rows 521..621 (viewport 521..534) — 100 missing, 0 already held · sorted by name Desc
people   dio      fetch #3 got 100 rows in 7.3s  ⚠ slow
```

The framework did not sort the rows that it holds. It sent the sort to the source, because the
source can order. When you hold 300 rows of 200,000, only the answer of the source is correct.

The request also became more expensive: 7.3s for 100 rows, against 3.0s for 200 rows. The source
must now order 200,000 rows for each window.

The same action against the source that can do nothing gives a different line:

```text
stubborn   sort   by name descending — the source cannot order; sorting the 3,000 rows held locally
```

No fetch follows. The cache holds all the rows, so the sort is a local operation. It is immediate,
and it is correct because the cache holds the complete set.

**The dangerous combination is a source that cannot order and a table that is too large to copy.**
The framework would then have to sort the rows that it loaded, and "sorted" would mean "the first
page of a sample".

Diorama does not do this. A paged view whose source orders the rows discards its cached row
positions instead of moving them, and it fetches the visible window again:

```text
people   scenery  row positions dropped — the source re-orders, so cached rows must be re-placed
```

The cost is one more fetch. The result is correct.

## Search, and the control that the UI hides

Search obeys the same rule:

```text
people   search   "smith" — pushed to the source
```

A source that cannot search gives the opposite line, and the framework filters the rows that it
holds. You see that line from the `stubborn` source, which holds all its rows.

The UI adds one condition. **A grid over a source that cannot search shows no search box.** The UI
hides the control instead of showing a control with a smaller scope than the user expects.

```admonish tip title="A grid has no search control"
A `grid` component shows no search box, and this does not depend on the source. Use `kind: binder`
for the full table controls.

A grid over a source that can search therefore offers less than the source can do.
```

## Latency multiplies the cost

Capabilities control *where* the work happens. Latency controls *what the work costs*.

| | fast source | slow source |
| --- | --- | --- |
| **can serve windows** | scrolling is almost free; waste is invisible | each unnecessary window is a visible delay |
| **cannot serve windows** | one large read, and it completes quickly | one large read, and the user waits for it |

The *count* of requests is the same in both conditions. **Measure the count, not the feeling.**

---

## What we covered

| Item | What it does |
| --- | --- |
| `capabilities: []` | Removes all operations except `list`. This gives the eager method. |
| the `source` line | Names what the source can do, and which load method follows |
| **paged** and **eager** | One window for each screen, against one read at open |
| the `sort` line | Says whether the source sorts, or the framework sorts the rows that it holds |
| the `search` line | The same, for filters. `kind: grid` hides the control. |
| `row positions dropped` | A paged view that refetches instead of showing a sample as the set |

```admonish tip title="What is next"
Capabilities explain the requests that the application had to send. The next section finds the
requests that it did not have to send, and adds a source that sends 19MB to fill a grid of five
columns.
```
