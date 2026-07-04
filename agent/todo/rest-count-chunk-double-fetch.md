# REST paged open: count + first chunk = double fetch

**Observed** (launch-control binder tab, 1-row relation):

```text
GET payload_flights/?mode=detailed&offset=0&limit=1&launch__id=…   ← count probe
REST count total=1
GET payload_flights/?mode=detailed&offset=0&limit=100&launch__id=… ← first chunk
```

Two requests; for a 1-row relation the same detailed record twice.

**Why:** diorama's paged loader asks `total_provider` (→ vista `get_count` →
`Api::fetch_total`, a `limit=1` GET reading the envelope's `total_key`)
BEFORE any chunk. The chunk GET's envelope carries the SAME total, but
`fetch_windowed` returns rows only and discards it. The lens's
`total_provider` has no sink, so the count's fetched row can't seed the
cache either (vantage-ui `crates/backend/src/lens.rs:build_paged_lens`).

**Constraints on the fix:**
- Sorted masters now push `?ordering=` server-side (can_order), so a
  "prefetch rows during count" hack regresses them: unsorted prefetch is
  wasted, the sorted chunk refetches anyway. `total_provider` doesn't see
  the scenery's sort.
- `mode=detailed` is baked into the table path, so even the `limit=1`
  count fetch pays for one fully-detailed record.

**Decided shape (data-first, count-last, capability-gated):**
1. A grid/list opens by fetching the FIRST PAGE (with the scenery's
   ordering), and a second page if needed to fill the viewport. Rows paint
   as soon as they land — no total gates first paint.
2. The count runs AFTER the data, and ONLY to size the scrollbar (how far
   scrolling can go). It runs only when the backend genuinely advertises a
   count capability (`can_count`).
3. REST has NO real count capability: a `limit=1` GET reading the
   envelope's `total_key` is not a count — stop issuing it. When window
   responses carry a `total_key`, read the total from the FIRST CHUNK's
   envelope for free; a first chunk shorter than its window IS the total
   even without one. Otherwise the scrollbar grows as pages load
   (unknown-total mode).

**Wins:** every paged open goes 2→1 requests; first paint no longer waits
on a count round-trip; binder relation tabs (small) are a single fetch;
refreshes stop re-paying counts.
