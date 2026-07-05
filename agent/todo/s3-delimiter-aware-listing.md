# vantage-aws: delimiter-aware S3 folder listing

Needed by vantage-ui's `kind: finder` composite (see vantage-ui
agents/plans/2026-07-05-finder-component.md).

`objects_table` today extracts only `<Contents>` from ListObjectsV2. Hierarchical browsing
sets `prefix` + `delimiter="/"`, and the response then splits children into two lists:
`<Contents>` (files) and `<CommonPrefixes>` (the "folders"). The second list is currently
dropped.

Wanted: a listing table that, given `prefix` + `delimiter`, merges both lists into one row
set with a synthesized `kind` column (`folder` for each CommonPrefix, `file` for each
Contents entry) and a `name` (the Key/Prefix with the current prefix stripped). Shape
target — the canonical finder row `{name, kind, size, modified}`; `size`/`modified` empty
for folders (S3 has no cheap folder aggregates; do NOT scan).

Prefer type-driven design over a bool flag on the existing table (per house rule): a
distinct listing model/table, not `objects_table(with_folders: true)`.

Constraint from the finder design: all per-prefix sceneries route to ONE Dio per
bucket/datasource, so a single refresh serves every open tree node. Pagination must stay
honest — ListObjectsV2's 1000-key pages surface as truncation, never auto-paged to
exhaustion.
