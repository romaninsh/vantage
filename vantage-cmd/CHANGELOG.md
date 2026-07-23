# Changelog

## 0.6.2 — 2026-07-23

- `CmdTableShell` implements `get_ref_target` (the bare relation target via the
  factory), matching `get_ref`. Previously the eligible-rows dropdown path
  returned `Unimplemented`.

## 0.6.1 — unreleased

- CBOR↔JSON helpers use the shared `vantage-types` walker: tagged values render their
  payload instead of collapsing to `null` through ciborium's serde bridge.

## 0.6.0 — unreleased

- Coordinated 0.6 release; internal dependencies realigned to 0.6. No public API changes.

## 0.5.2 — 2026-06-07

- Reuse the rhai `Engine` and compiled `AST` across calls instead of rebuilding
  and re-parsing per call (removes the N re-parses a per-row detail loop incurred).
- Two-role scripts: a table can declare a `list` script (the default) and a
  separate `detail` script. `get_table_value(id)` runs the detail script with the
  id — and the cached list-pass row — injected into scope (`row` map in
  `QueryContext`), so the detail script can read cheap columns already fetched.
  `Vista::get_value` routes through the detail script; `get_table_value_with_row` /
  `get_vista_value_with_row` feed the cached row down.
- Surface child-process stderr into the tracing log (WARN on non-zero exit, DEBUG
  otherwise) so tool diagnostics are visible to the host app.

## 0.5.1 — 2026-06-07

- `Cmd::with_base_dir` — set a base directory for the data source. A relative
  `command` path containing a separator (e.g. `./scripts/gh-stats.py`) is
  resolved against it; bare names (`gh`) stay on `PATH` and absolute paths pass
  through. When set, every table's child process also runs with this directory
  as its working directory, so a script can resolve sibling files relative to
  it. Lets an inventory ship its own helper scripts and reference them by a
  path relative to the inventory root.

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
