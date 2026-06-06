# Changelog

## 0.5.0 — 2026-06-06

First release — incubating. A read-only persistence backend that fetches data by **running a local
command**: the binary and environment are locked on the `Cmd` data source, and a per-table
[Rhai](https://rhai.rs) script builds the argv, runs the command, and parses its output into rows.

- `Cmd` data source — locks a command (e.g. `aws`, `kubectl`, `mercury`) plus a declared
  environment; `with_pass_path` controls whether `PATH` / `HOME` are forwarded so the binary
  resolves. Cheap to clone (`Arc`-backed).
- Per-table scripts run with the current query in scope (`conditions`, `columns`, `limit`, `offset`,
  `id_column`) and three registered helpers: `run(args)` (executes the locked command, returns
  `{ stdout, stderr, exit_code }`), `parse_json`, and `parse_jsonl`. The script returns an array of
  rows. The subprocess call runs on a blocking thread so the async runtime is never blocked.
- `TableSource` impl is read-only: `list` / `get` / `count` go through the script; writes and
  aggregations (`sum` / `min` / `max`) error loudly. Relations resolve via deferred eq-conditions,
  so `with_one` / `with_many` traversal pushes the parent's key into the child script's
  `conditions`.
- Vista integration: `CmdVistaFactory` builds a `Vista` from a typed `Table<Cmd, _>` or from YAML
  (`CmdVistaSpec` — the `cmd:` block carries the Rhai script and optional command / env overrides).
  `CmdModelFactory` bundles example AWS-CLI vistas.
- `examples/aws-cli.rs` drives the `aws` CLI through the Vista CLI runner; integration tests cover
  the script / condition path against shell fixtures.
