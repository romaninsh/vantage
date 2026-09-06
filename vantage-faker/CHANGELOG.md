# Changelog

## 0.6.15 — 2026-09-05

- The `rhai` effect compiles its mutation script **once**, when the effect
  starts, and runs the compiled script per tick on a bounded `vantage-rhai`
  host — before, a fresh engine was built on every tick, up to 50 Hz. A
  script that does not parse now fails at start instead of on every tick.
- `FakerVocab(Arc<FakerCtx>)` carries the verbs as a `Vocab`.

## 0.6.14 — 2026-08-16

- The shaped and live-folder shells implement `preview_query`. Shaping forwards
  to the shell it wraps, since latency and injected faults do not change the
  query; the live-folder shells report the local path they read.

## 0.6.13 — 2026-07-30

- **A faker table can now be ordered.** `build` and `build_shaped` seeded the
  store but never registered the columns as `VistaMetadata`, so the shell
  reported none and `Vista::add_order` failed every sort with "Unknown column"
  — on a shape that advertised `order`. Because the failure was instant and the
  rows never landed, each viewport movement re-requested them, which presented
  as a hung window. Generated columns are now declared, orderable and
  searchable, carrying through the `id`/`title`/`searchable` flags they were
  given.
- Track vantage-diorama 0.12.

## 0.6.12 — 2026-07-30

- Track vantage-diorama 0.11. No behavior changes here.

## 0.6.11 — 2026-07-30

- **`BackendShape` / `ShapedShell`** — one struct is a whole backend
  personality: advertised `capabilities` (anything unadvertised refuses as
  unsupported), `page_size`, per-operation-class latency (`list`/`get`/
  `window`/`count`, plus `search_extra` while a filter is active), a
  `FaultSchedule` (`error_rate`, scheduled `offline` windows, `cursor_expiry`,
  `total_lie`, `boundary_skew`), undeclared fat `extra_fields`, `weirdness`
  (a share of string cells drawn from an anomaly pool), and a `seed` for
  deterministic replay. `FakerTable::build_shaped` reads the same store
  through the shaped shell.
- **`RhaiEffect`** (feature `rhai`) — a scripted mutation loop: seeds `count`
  rows, then evaluates a Rhai script every `interval` with mutation verbs
  (`ids`, `get`, `set`, `patch`, `insert`, `delete`, `fake`, `rand_int`,
  `rand_float`, `pick`); every verb writes the store and broadcasts the
  matching `ChangeEvent`.
- `ValueGen::seeded` and `with_weirdness` — deterministic, anomaly-capable
  value generation; the effect rng is decorrelated from the value rng.
- Every tolled operation on a shaped shell emits a `tracing::debug!` under
  `vantage_faker::shape` — the tap request accounting reads.

## 0.6.10 — 2026-07-28

- Track vantage-diorama 0.10. The requirement stayed at `0.9` when diorama
  released 0.10, which left this crate unresolvable and — more to the point —
  put a second diorama into the graph of anything depending on both. `Dio` and
  `ChangeEvent` from two versions are different types, so that surfaces far from
  its cause.

## 0.6.9 — 2026-07-27

- Track vantage-diorama 0.9 (client-side search and the augmentation spec layer
  removed). No behavior changes here.

## 0.6.8 — 2026-07-24

- Track vantage-diorama 0.8 (draft-servo release). No behavior changes.

## 0.6.7 — 2026-07-23

- `FolderListingShell` implements `get_ref_target` for the `subdir` relation —
  the bare listing rooted at the shell's own path — so the eligible-rows
  dropdown path no longer returns `Unimplemented`.

## 0.6.6 — 2026-07-22

- Track vantage-diorama 0.7 (Servo + ChangeFlash). No faker API changes —
  the crate only consumes `ChangeEvent`, which is unchanged.

## 0.6.5 — 2026-07-15

- Track vantage-diorama 0.6.17: the `scenery_cli` example calls
  `notify_dataset_changed()` (renamed from `invalidate_all()`). No API changes.

## 0.6.4 — 2026-07-05

- Listing rows leave a FOLDER's `size` unfilled (absent) instead of reporting 0:
  a folder's recursive size is the augment's to fill (diorama's gap rule fetches
  exactly those rows), and consumers render the absence as blank, never a lying
  zero. File rows keep carrying their own size. Requires vantage-diorama 0.6.16.

## 0.6.3 — 2026-07-05

- `LiveFolderSim::size_augment()` — the folder-size vista packaged as a dio-level
  augment (`Detail::Fixed`, keyed by the listing's hidden `path` column, merging
  `{size, file_count}`). A listing Dio built with it patches folder rows in place
  as hydration lands, with the size vista's file-count-scaled latency intact —
  one Dio, no second observation anywhere above it. Requires vantage-diorama
  0.6.15.

## 0.6.2 — 2026-07-03

- `LiveFolderSim`: a synthetic, constantly-mutating multi-layer log tree. One shared run loop
  simulates three streams under `{date}/` — an `access_logs_HH` chunked access log (active chunk
  bumps each second at `requests_per_sec × bytes_per_request` bytes; rolls a new `chunk_NN.log` when
  it crosses `chunk_threshold`, sized for ~100 files/hour at defaults), a rare `error_logs` stream
  (one `HH:MM:SS-errors.log` file per error occurrence, gated by `error_pct_per_sec`), and ten
  `events/<type>.log` event files each with its own 1–10% per-second probability of a 2000–4000-byte
  bump. Folders and files carry `created`/`modified`; any leaf mutation touches every ancestor up to
  root. A `backfill` duration replays the algorithm at full speed from `now − backfill` to `now` on
  construction. The listing vista is a `FolderListingShell` reading the live tree on every list,
  declaring a `subdir` HasMany reference so a Dio over a parent folder can traverse into any child
  via `get_ref("subdir", row)`. The folder-size vista is get-only (no list) and fetches with a
  100ms–1s latency scaled by file count — for exercising viewport debounce.
- `live_folder_cli` and `scenery_folder_cli` examples: the former renders the whole tree as a
  `tree(1)`-style outline; the latter opens three reactive `TableScenery` panes (ymd, error_logs,
  events) wired through `Dio::get_ref("subdir", ...)` and refreshed on every sim tick.

## 0.6.1 — 2026-07-02

- `PulseSim`: a generic, config-driven "live aggregate feed". One shared run loop drives three
  coupled tables — a raw `Feed` append log (`{key, delta, updated}`, newest-first, expiring after
  `feed_retention` so the stream visibly flows; `Inserted`/`Deleted`), a derived `Aggregate`
  keyed-upsert (`{key, value, vs_baseline, live}`; `Updated`), and a `Minutes` arrivals time series
  (`{minute, attendees}`, one bucket per `bucket` window, summing only arrivals/positive deltas,
  kept to the last `minutes_window` buckets) — so a subscribed Dio applies changes in place. Per-key
  values mean-revert within a rubber-banded ±`band_pct` of a configured baseline; each key re-fires
  on its own random interval (bursts, never a whole-interval sleep); designated keys periodically
  blip offline. Keys, baselines, rates, retention, buckets, column names, and offline set are all
  config (`PulseConfig`).

## 0.6.0 — 2026-07-01

- Initial release. Synthetic, optionally-live datasource for Vantage.
- `StaticEffect` generates rows once; `FifoEffect` inserts newest-first and expires each after a
  random retention, broadcasting `ChangeEvent`s so a subscribed Dio animates inserts/expiries
  without re-listing.
- Name-aware value generation via the `fake` crate (email, name, phone, city, …) with a type
  fallback (int, decimal, bool, datetime, string).
- `fifo_cli` and `scenery_cli` examples.
