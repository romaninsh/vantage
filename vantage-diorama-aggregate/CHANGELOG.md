# Changelog

## 0.6.5 — 2026-07-30

- Track vantage-diorama 0.11.
- **`"aggregate recompute"` joins the debug stream.** An aggregate has no
  datasource of its own to opt in — its debug lines are inherited from
  whatever the source Dio was built with, and fire under the same
  `vantage_diorama::debug` target with the same `ds=` the source uses.
  Reports the trigger (`initial`, `debounce-trailing`, `nudge`, `lagged`, or
  the `DioEvent` variant that woke the loop), rows in and out, timing, and
  whether the output changed enough to publish. A `derive()`'s first load
  reports two `initial` lines — the eager compute that seeds the derived
  Vista's schema, then the engine's own seed pass reading that output back —
  which is the intended reading, not a duplicate line.

## 0.6.4 — 2026-07-28

- Doc fixes: three intra-doc links that rustdoc could not resolve under
  `--document-private-items` — one to a crate that isn't a dependency here, one
  missing its crate path, one restating a target its label already resolved to.
  No code change.

## 0.6.3 — 2026-07-28

- Track vantage-diorama 0.10.
- **A paged source no longer leaves every aggregate at zero.** The engine
  ignored `RangeLoaded`, and a chunk load emits no per-row event — so on a cold
  cache the figures derived from a paged table never recomputed and sat at their
  starting values. They now recompute as each window lands, which is what makes
  a figure over a paged table climb as its pages arrive rather than appearing
  finished and wrong.

## 0.6.2 — 2026-07-27

- Track vantage-diorama 0.9. The facade Vista no longer lifts `can_search`, so a
  search over a source Dio must be one the master can answer; ordering and
  conditions are unaffected. A derived aggregate holds its whole output in
  memory and keeps advertising all three.

## 0.6.1 — 2026-07-26

- Pins diorama 0.8.4, whose facade Vista now honours condition / order / search.
- Adds coverage for many independent aggregations over one source Dio: several
  values plus a derived Dio, across two lenses, all reacting to one change, with
  one dropped mid-run to show the survivors keep tracking. Each engine holds its
  own bus receiver and its own cache table, so aggregates compose over a Dio the
  same way views do.

## 0.6.0 — 2026-07-26

First release. Reactive client-side aggregation over a Diorama: derive a count,
a total, or a grouped table from a `Dio` and keep it current as the source
changes.

- **`Aggregation` — one trait, one method.** `compute(&self, rows: &Rows) ->
  Output`, where `Rows` is exactly what
  `ReadableValueSet::list_values` already returns. Implementing it is the whole
  cost of adding a new derivation; nothing in the engine changes.

- **Two surfaces, picked by the output type.** An aggregation producing a
  `CborValue` becomes an `Arc<dyn ValueScenery>` — the shape a UI already mounts
  for a counter or a total. One producing `DerivedRows` becomes a full `Dio`,
  with its own Vista, cache table and sceneries.

- **Recomputation, not incremental updates.** Every change recomputes the whole
  aggregation; there is no accumulator and no delta plumbing. With no retained
  state there is nothing to drift out of step with the source, a dropped change
  notification is a non-event, and reductions that cannot retract a row (`max`,
  `min`, `distinct`) need no special case. Two things bound the cost: the engine
  debounces, so a burst collapses into one recomputation plus one flush; and
  every output is compared against the last before anything is published, so
  recomputing often does not mean repainting often.

- **A derived Vista advertises what it can actually do.** Unlike its source it
  holds its entire row set, so it reports `can_order` / `can_search` /
  `can_count` and honours them — sorting a derived table is ordinary push-down
  into its own Vista, not a client-side pass over partial data. Clones carry
  their own query state, so two views can sort one aggregate differently. Writes
  are refused: derived rows are a function of the source and have nowhere to
  land.

- **`AggregateLens`** owns the aggregate datasource — one cache file, one table
  per aggregate. Derived results are reproducible, so the cache buys a warm
  start rather than durability: a restart paints the last known value and
  recomputes behind it. `retain(&[names])` drops the tables of aggregates that
  are no longer declared.

- **Refresh is pushed down, not simulated.** `request_refresh` on an aggregate
  asks the *source* to go and look again, because recomputing over unchanged
  rows would only produce the same answer. The resulting events come back up the
  bus and drive the recomputation.

- **A refresh never flashes a derived value through empty.** `Dio::refresh`
  emits `Refreshing` before running its callback, and the common callback shape
  clears the cache before refilling it — so the engine waits for the trailing
  `DatasetChanged` rather than reading the source mid-clear.

- Built in: `Count`, `CountWhere`, `Sum`, `Avg`, `Min`, `Max`, `Distinct`, and
  `GroupBy` with a caller-supplied `GroupReducer` (`Reduce` wraps a closure).
  Numeric comparison is numeric, never a debug-string compare; ties in a sort
  break on id, so ordering is reproducible.
