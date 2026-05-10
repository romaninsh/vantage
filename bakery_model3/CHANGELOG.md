# Changelog

## 0.4.1 — 2026-05-10

- `cli-vista` example gains a `surreal` source, joining `csv`, `sqlite`, `postgres`, and `mongo`. Run e.g. `cargo run --example cli-vista -- surreal bakery list` against the v2 bakery database (or override with `SURREALDB_URL`).
- `cli` example clarifies its role: it stays on `AnyTable` because it demonstrates `ref` traversal, which Vista doesn't surface yet. For Vista-backed work, prefer `cli-vista`.
- Pulls in [`vantage-surrealdb 0.4.5`](https://docs.rs/vantage-surrealdb/0.4.5/vantage_surrealdb/) with the `vista` feature enabled, so the bakery models work uniformly across every Vista driver in the workspace.
