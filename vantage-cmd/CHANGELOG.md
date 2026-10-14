# Changelog

## 0.7.0 — 2026-10-14

### Breaking changes

- The shell implements `TableShell` 2.0 and advertises `Window` and `Traverse`; there is no
  cursor and no write.
- A DETAIL script answers `get(id)` only with the row keyed by `id`, or with a single id-less row
  (stamped with `id`); the first-row fallback is gone.
- On a narrowed set, `get` returns nothing for ids outside the set.
- The shell no longer passes a listed row to the DETAIL script; diorama's `Fetch::Custom` hands
  it over. `Cmd::get_table_value_with_row` stays.
- A condition takes `Eq` on a declared column; any other operator is `unsupported(FilterOps)`, and
  an unknown column is an error.
- Join traversal is strict. `iam.user_groups` and `iam.user_policies` declare a hidden `UserName`
  column.
- The bundled vistas flag their foreign-key and request columns `[hidden, request_param]`, so rows
  needn't carry them.
- A client-side filter keeps a row lacking the field only for a `request_param` column or an
  undeclared field; a `hidden` column is a row field.
- A dotted path into a declared column is refused as `Unimplemented`.
- `preview()` is `{query}`: the scope JSON, with no `note`.
- A bundled spec declaring a contained relation makes `for_name` return `None`.

### Other changes

- Vista 2.0 driver cleanup: builds against vantage-vista 2.0.0 and vantage-table 2.0.0.
- `fetch_window` slices the listed rows and sets `total`, since a script may ignore the scope's
  `limit` and `offset`.

Migration guide: https://vantage-ui.com/book/migration-2.0.html

## 0.6.10 — 2026-10-07

- `ExprDataSource` builds on `vantage_expressions::execute_by_resolving` and `defer_execute`.

## 0.6.9 — 2026-10-07

- Builds against the bumped vantage crates (edition 2024 workspace).

## 0.6.8 — 2026-10-04

- Parameters resolve through `vantage_expressions::resolve_param`.

- The vista factory uses the shared spec helpers.
- Depends on `vantage-expressions` 1.0.1, `vantage-table` 1.1.1, `vantage-vista` 1.1.1.

## 0.6.7 — 2026-10-03

- Spec `lazy:` columns become Vista computed columns.
- Depends on `vantage-table` 1.1.0, `vantage-vista` 1.1.0.

## 0.6.6 — 2026-10-03

- Depends on `vantage-core` 1.0.0, `vantage-types` 1.0.0, `vantage-expressions` 1.0.0,
  `vantage-dataset` 1.0.0, `vantage-table` 1.0.0, `vantage-vista` 1.0.0, `vantage-rhai` 1.0.0,
  `vantage-cli-util` 0.6.2.

## 0.6.5 — 2026-09-05

- Scripts run on a `vantage-rhai` host. The direct `rhai = "1.21"` pin — the
  last 1.21 anywhere in the family — is replaced by `vantage-rhai`, so the
  engine version is the one every other crate uses.
- `CmdVocab { command, env, pass_path, base_dir }` is the security lock as a
  `Vocab`: `run(args)` can only ever spawn the configured command with the
  configured environment. `CompiledScript` compiles the script once on a
  bounded (`Limits::background()`) host; each evaluation binds `conditions`,
  `columns`, `limit`, `offset`, `id_column`, `id` and `row` through an `Env`.

## 0.6.4 — 2026-08-16

- `CmdTableShell` implements `preview_query`. A cmd table's argv is assembled
  inside its Rhai script, by the same call that runs it, so no preview can render
  the argv without running the command. It reports what decides the argv instead:
  the locked program, the script, and the scope values it would be evaluated
  with.

## 0.6.3 — 2026-08-12

- **Bug fix:** `CmdTableShell::get_ref_target` resolves relations whose target
  constructor returns a typed table. It went through an entity-typed downcast
  that failed, so a reference field's list of eligible rows came back as an
  error.

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
