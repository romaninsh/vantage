# diorama: dio-level column augmentation from a secondary vista

Shipped in PR #337 (vantage-diorama 0.6.15 `Detail::Fixed` + vantage-faker 0.6.3
`LiveFolderSim::size_augment`): the pre-existing two-pass augmentation already
carried the laziness (viewport-driven detail pass), coalescing (per-id
single-flight), in-place patching, and modified-driven refetch (refresh
reconciliation demotes rows whose list fields moved); the missing piece was a
detail source that is a fixed get-only Vista handle rather than a catalog name.

Needed by vantage-ui's `kind: finder` single-Dio invariant (vantage-ui
agents/plans/2026-07-05-finder-component.md, rule 6).

A Dio should be able to AUGMENT its rows with columns fetched from a secondary, get-only
vista, keyed by one of the row's columns. Canonical case: vantage-faker's live_folder —
the listing vista supplies `{name, kind, modified}`; the size vista supplies
`{path → size, file_count}` with deliberate 100ms–1s latency. The dio pulls the augment
lazily per row (folders only, keyed by path), patches the row via its normal ChangeEvent
path when the value lands, and re-pulls when the base row's `modified` moves.

Consumers (sceneries, vantage-ui observations) see ONE dio whose rows simply have the
extra columns — no second data handle anywhere above the dio. Sources without the augment
just lack the columns (S3: no folder sizes; column absent, not zero).

Design notes:
- Prefer type-driven: an Augment descriptor the Dio is built with (lens-level wiring), not
  a bool/callback bolted on.
- Laziness + coalescing are the point: only visible/hydrated rows fetch; slow gets must
  not stall listing; per-row staleness (base row touched → augment refetch).
- This is also the debounce showcase: the faker size vista's file-count-scaled latency
  exists precisely to exercise this path.
