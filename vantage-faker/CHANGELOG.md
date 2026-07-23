# Changelog

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
