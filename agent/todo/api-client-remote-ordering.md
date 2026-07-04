# REST api-client: opt-in remote ordering

**Today:** sorting is purely local — the scenery orders the Dio cache
(`set_sort`), so only the hydrated viewport is ordered. `vantage-api-client`'s
REST side has no ordering plumbing at all; GraphQL's `add_order_by`
deliberately drops the order. On a 535-row table with a 200-row viewport,
"sort by net desc" is correct within the window, not globally.

**Want:** per-table opt-in in the datasource/table YAML: which columns are
remotely orderable and the query-arg convention (e.g. `?ordering=-net`,
LL2-style). The vista then advertises `can_order` for those columns; sceneries
push the sort into the fetch; everything else keeps local sort.

**Consumers:** vantage-ui shaped bindings (`scenery("launches").sort(...)`)
and grid header clicks — both already call the same scenery sort seam, so
they'd gain global ordering with no UI changes.
