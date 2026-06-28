# Changelog

## 0.6.0 — 2026-06-28

Initial release — a native Kubernetes `TableSource` backend for Vantage,
incubating in the `vantage` repo (workspace-excluded) and published to
crates.io. Versioned at 0.6.0 to align with the rest of the `vantage-*` 0.6
line (matching `vantage-aws`). Modeled on `vantage-aws`.

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
