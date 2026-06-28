# vantage-kubernetes

A native Kubernetes API backend for the [Vantage](https://romaninsh.github.io/vantage)
framework — **incubating**.

It treats the Kubernetes API as a Vantage `TableSource`: nodes, pods,
deployments, services and friends become drillable, relatable tables that any
Vantage consumer (the `k8s-cli` example, the Vantage UI, an agent over Vista)
can browse. Built on [`kube`](https://crates.io/crates/kube) for
config/auth/TLS; resource objects are fetched as raw JSON and **projected**
into flat, typed records so columns, relations, and charts all just work.

> Incubated in the `vantage` repo and excluded from the workspace (like
> `vantage-aws`). Build and test it from this directory.

## What it does

- **Resources as tables.** Each table's name is its API path (`api/v1/pods`,
  `apis/apps/v1/deployments`). A per-resource projector flattens the nested
  object (`metadata.name`, `status.phase`, `spec.nodeName`), derives
  array-backed fields (`ready` "2/3", restart counts, node IPs), and parses
  K8s quantities (`16331752Ki`, `250m`) into numbers.
- **Relations.** `namespace → pods/deployments/services/…`, `node → pods`,
  `deployment → replicasets → pods`. The headline `deployment → pods` drill is
  resolved by recovering the owning Deployment from each pod's ReplicaSet
  name, so it narrows correctly without relying on label conventions.
- **Read-only (v0).** Listing, counting, filtering, and relation traversal.
  Writes (scale/restart/delete) are a later phase.

## Quick start

```bash
# 1. A local cluster + sample workloads (nginx ×3, a Job, a 2-container pod):
./scripts/start.sh        # minikube + metrics-server
./scripts/ingress.sh      # helm upgrade --install the fixtures into `demo`

# 2. Browse it with the CLI:
cargo run --example k8s-cli -- core.pods
cargo run --example k8s-cli -- core.pods namespace=demo
cargo run --example k8s-cli -- apps.deployment name=web :pods      # drill to pods
cargo run --example k8s-cli -- core.nodes =name,cpuCapacityMillicores,memCapacityBytes
cargo run --example k8s-cli -- --format=json metrics.node_metrics

# 3. Tear down:
./scripts/stop.sh
```

The CLI connects via your current kubeconfig context (honours `$KUBECONFIG`).
Its argument grammar (`field=value`, `[N]`, `:relation`, `=cols`, `@count`,
`--format`) comes from `vantage-cli-util`'s `vista_cli` — see `k8s-cli --help`.

## Using the crate

```rust,no_run
use vantage_kubernetes::KubernetesCluster;
use vantage_kubernetes::models::core::pods;

# async fn run() -> vantage_core::Result<()> {
let cluster = KubernetesCluster::from_default().await?;       // current kubeconfig
let pods = cluster.vista_factory().from_table(pods::pods_table(cluster.clone()))?;
let rows = pods.fetch_window(0, 100).await?;                  // Vec<(id, record)>
# Ok(()) }
```

## Resources

`nodes`, `namespaces`, `pods`, `deployments`, `replicasets`, `services`,
`configmaps`, `secrets` (metadata only), `jobs`, `events`, plus
`metrics.node_metrics` / `metrics.pod_metrics` (require the metrics-server
addon). The set expands mechanically — add a module under `src/models/` with a
`PATH`, a `*_table` constructor, and a `project` function, then register both
in `src/models/mod.rs`.

## Testing

```bash
cargo test --lib                       # offline: quantity/datetime/projector

./scripts/start.sh && ./scripts/ingress.sh
RUN_K8S_INTEGRATION=1 cargo test --test '*' -- --test-threads=1   # live cluster
```

Integration tests skip cleanly when `RUN_K8S_INTEGRATION` is unset or no
cluster is reachable. CI (`.github/workflows/kubernetes.yaml`) stands minikube
up, deploys the Helm fixtures, and runs the full suite.

## Publishing

Crates.io publish is deferred until the `vantage-*` path dependencies are
released at compatible versions (same situation as `vantage-aws` today). The
crate pins a `k8s-openapi` version feature because `kube-client` requires one;
we don't use its generated types (everything goes through raw JSON).

## License

MIT OR Apache-2.0.
