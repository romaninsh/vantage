# Changelog

## 0.7.1 — 2026-10-18

- Requires vantage-table 3.0.0.

## 0.7.0 — 2026-10-14

### Breaking changes

- The shell implements `TableShell` 2.0 and advertises `Order`, `Search`, `Window` and
  `Traverse`: the page-number `fetch_next`, `fetch_page` and `set_page_size` are gone.
- A condition takes `Eq` on a declared column; any other operator is `unsupported(FilterOps)`, and
  an unknown column is an error.
- `set_order` on an unknown column is an error.
- Join traversal is strict. Pods declare `ownerName` and `ownerDeployment`, and replicasets
  declare `ownerName`, as hidden columns, so the deployment and replicaset drills narrow.
- A dotted path into a declared column is refused as `Unimplemented`.
- Quicksearch no longer matches the row id. It is a case-insensitive substring of any cell's
  text, numbers, bools and nested values included.
- Quicksearch reads the columns flagged `Searchable`, or every column when none is (every field
  when the table declares no columns). No column is flagged by default, and a table that declares
  columns no longer searches undeclared fields.
- Ordering ties break on id, and null cells sort last in both directions.

### Other changes

- Vista 2.0 driver cleanup: builds against vantage-vista 2.0.0 and vantage-table 2.0.0.
- `get` and `count` honour the search.
- `fetch_window` slices the listing and sets `total`.

Migration guide: https://vantage-ui.com/book/migration-2.0.html

## 0.6.5 — 2026-10-07

- `ExprDataSource` builds on `vantage_expressions::execute_by_resolving` and `defer_execute`.

## 0.6.4 — 2026-10-04

- Parameters resolve through `vantage_expressions::resolve_param`.

- Vista metadata comes from `Table::vista_metadata`.
- Depends on `vantage-expressions` 1.0.1, `vantage-table` 1.1.1, `vantage-vista` 1.1.1.

## 0.6.3 — 2026-10-03

- `Table::add_computed_column` columns reach the Vista.
- Depends on `vantage-table` 1.1.0, `vantage-vista` 1.1.0.

## 0.6.2 — 2026-10-03

- Depends on `vantage-core` 1.0.0, `vantage-types` 1.0.0, `vantage-expressions` 1.0.0,
  `vantage-dataset` 1.0.0, `vantage-table` 1.0.0, `vantage-vista` 1.0.0, `vantage-cli-util` 0.6.2.

## 0.6.1 — 2026-08-16

- `KubeTableShell` implements `preview_query`, naming the collection it lists and
  reporting that search and ordering are applied to the returned rows rather than
  pushed to the API server.
- **Version:** this crate jumps from 0.1.x to the 0.6 line its dependents already
  require. The old number could never satisfy a `0.6` requirement, so consumers
  silently resolved the registry copy instead of a local path patch.
- `KubeTableShell::preview_query` reports the shell's `page_size`.

## 0.1.2 — 2026-08-12

- **Bug fix:** `KubeTableShell::get_ref_target` resolves relations whose target
  constructor returns a typed table. It went through an entity-typed downcast
  that failed, so a reference field's list of eligible rows came back as an
  error.
- The vantage dependencies carry a `path` next to their version, like
  `vantage-aws` and `vantage-cmd`. This crate sits outside the workspace, so it
  used to build against whatever crates.io last published — a change made here
  and in `vantage-table` in one commit could not compile together. It now builds
  against the sources beside it, and still publishes by version.

## 0.1.1 — 2026-07-23

- `KubeTableShell` implements `get_ref_target` (the bare relation target via the
  factory), matching `get_ref`. Fills the eligible-rows dropdown path that
  previously returned `Unimplemented`.

## 0.1.0 — 2026-06-28

Initial release — a native Kubernetes `TableSource` backend for Vantage,
incubating in the `vantage` repo (workspace-excluded) and publishable to
crates.io. Modeled on `vantage-aws`.

### Added

- `KubernetesCluster` — wraps a `kube::Client` (kubeconfig / in-cluster auth +
  rustls), with a default namespace. Installs the ring crypto provider once.
- Native API access: resources are fetched as raw JSON and run through a
  per-resource **projector** that flattens nested objects, derives array fields
  (`ready` "2/3", restart sums, node addresses), parses quantity strings
  (`"16331752Ki"`, `"250m"`) into numbers, and assigns stable ids + join keys.
- `impl TableSource for KubernetesCluster` with post-fetch filtering, so nested
  and label/owner-derived fields can drive relations.
- Resource models: pods, nodes, namespaces, services, configmaps, secrets,
  events, deployments, replicasets, jobs, and `metrics.k8s.io` node/pod metrics,
  with `with_many` relations (namespace → children, node → pods,
  deployment → replicasets → pods).
- Vista stack: `KubeVistaFactory` + `KubeTableShell` implementing ordering,
  quicksearch and pagination client-side over the materialized listing;
  capabilities advertise count/order/search/page/window. K8s-specific Rhai
  helpers.
- `k8s-cli` example (mirrors `aws-cli`) for listing/filtering/traversing live
  resources.
- minikube + Helm integration-test harness (`scripts/{start,ingress,stop}.sh`)
  and a `.github/workflows/kubernetes.yaml` CI job; integration tests gated by
  `RUN_K8S_INTEGRATION`.

### Notes

- Depends on the crates.io `vantage-*` 0.6 line (no path deps), so its types
  unify with downstream consumers without a `[patch]`.
- Read-only for v1: writes/actions, server-side selector pushdown, and the
  expansion resource set are follow-ons.
