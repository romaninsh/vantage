# Rhai unification — live status

Living status for the work in `/Users/rw/.claude/plans/ok-plan-out-the-woolly-twilight.md`
(the approved plan; a copy of its phase structure is summarised here so this file stands on
its own). **Update this file at every commit.** A chat session was lost on 2026-09-05 with
~40 files uncommitted and no notes; this file exists so that cannot cost a day again.

Branches: `rhai-unification` in vantage and in vantage-ui. Nothing ships between phases;
one vantage release, then one vantage-ui release pinned to it.

## Phase 1 — Loader, includes, serde_yaml_ng (vantage-ui) — DONE

Commit `40f55cf` on vantage-ui `rhai-unification`: one loader in `ui-grammar`
(`Document`, `load_document`, `validate_into`, `LazyLock` schema cache), `include.rs`
(364 lines, `!include` with root confinement, cycles, cache), `Leaf.includes` + watcher
reverse lookup, `config.rs` and both legacy bins deleted, `ShapedObserveRef::File` removed,
`serde_yaml` gone (0 references). Tests: ui-grammar 20, inventory 6+6+30, catalog 1+1 pass.
`cargo check --workspace` in vantage-ui currently fails only because the local vantage
checkout (patched in) does not compile — see Phase 2.

Not yet done from Phase 1: 1.6 docs (`# language=rhai` convention in scaffold READMEs and
`rhai-expressions.md`). Folded into the final sweep.

## Phase 2 — Data layer (vantage) — IN PROGRESS, uncommitted

Mechanical part done in every crate: `rhai = "1.25"` → `vantage-rhai = { path }` under the
`rhai` feature, all `rhai::` paths → `vantage_rhai::rhai::`. vantage-cmd's 1.21 pin is gone.

| Crate | State |
|---|---|
| 2.1 vantage-vista | **Done.** `ConventionalVocab`, `ShellVocab`, `FetchVerbs`; `eval_*` take `&Host` + `Env`; `run_script`/`preview_script` on `Limits::background()` hosts; `lazy_value_closure` compiles once (`(&str) -> Result<LazyValueFn>`, both callers updated); `augment_source_closure` builds its host once; `convert.rs` public + arrays/maps, `dynamic_to_json` replaced by `vantage_rhai::to_json`; `TableShell::rhai_env` added. 60+4+5 tests. |
| 2.2 vantage-sql | **Done.** `register_engine!` emits `SqlVocab` + a per-dialect `LazyLock<Host>` (`__host()`) and imports `rhai` into the invoking scope; `__create_engine` gone; `eval_to_select_args` uses `Env` (`args`, `base`) and `Block` (cached); `eval_to_select` collapsed onto it. Tests/example/smoke moved. `e6.err` fixture reworded to the host's `unknown name` message. 739 tests. |
| 2.3 vantage-surrealdb | **Done.** `SurrealVocab`; `me` moved from an `on_var` hook to `surreal_env` / `TableShell::rhai_env` (the hook collided with the host's resolver hook); `register_surreal_engine!` emits `__host()`; traversal, modify and query-source sites on hosts, vendor-before-conventional order kept; tests for `me` + resolver coexistence and bounded scripts. 103 tests. Fixture runner: 16/18, the 2 mismatches (`v4_q07/08`, `'x'` vs `"x"` quoting) fail identically at HEAD — pre-existing drift, not this work. |
| 2.4 vantage-cmd | **Done.** `CmdVocab { command, env, pass_path, base_dir }` is the security lock as a `Vocab`; `CompiledScript { script: Compiled<Block> }` on a `Limits::background()` host; `eval` builds an `Env`. 19 tests. |
| 2.5 vantage-faker | **Done.** `FakerVocab(Arc<FakerCtx>)`; the effect compiles its script once (a script that does not parse never ticks) and runs the `Compiled<Block>` per tick under `spawn_blocking`; background limits. Its scalar CBOR converters stay (they tolerate rather than error; switching them to vista's is a behaviour change left for later). 44 tests. |
| 2.6 vantage-diorama | **Done.** `ServoVocab`; `cbor_to_dynamic`/`record_to_map` re-exported from vista, `dynamic_to_cbor` keeps only the chrono-instant case and delegates the rest; `rhai` feature now pulls `vantage-vista/rhai`. Test moved onto a host. All suites pass. |
| vantage-api-client | `lazy_value_closure` caller updated. Compiles. |
| vantage-rhai | Grew during this phase (unpublished, part of the release): `Compiled<Block>` has `eval_as`/`eval_bool`; parse errors keep rhai's "Syntax error:" prefix. 50 tests, clippy clean. |

**Phase 2 verification:** `Engine::new()` outside test modules exists only in
`vantage-rhai/src/host.rs`; `set_max_expr_depths` / `on_var` only in `vantage-rhai`; no crate
declares a direct `rhai` dependency except `vantage-rhai`.

vantage-ui against this data layer: one break, `crates/backend/src/connect/relations.rs`
`narrow_via_script` (old `eval_ref_script` signature) — patched minimally so the workspace
builds; Phase 3.5 deletes it with `ReferenceFull.rhai`.

**Incident 2026-09-05:** a `git stash` / `stash pop` used to check a fixture baseline stranded
the whole tree in the stash (background cargo rewrote `Cargo.lock` between them). Recovered;
lesson recorded in memory. Use a worktree for baselines, never a stash here.

## Phase 3 — vantage-ui sites, dead slots, scenery direct reads — IN PROGRESS

Two commits' worth of work on vantage-ui `rhai-unification`, the first staged and waiting
on a 1Password unlock to sign (index intact; the user's working `Cargo.lock` is preserved at
`/tmp/vui_worklock` and restored after the commit lands).

| Item | State |
|---|---|
| 3.5 six slots | **Done.** `expressions:`, `unit.rhai`, column `expr`/`lazy`, `references.rhai`, page column `render`/`copy` deleted end to end (inventory structs, schema column, grid cell/clipboard paths, loaders, `UiRelation.rhai`). `narrow_via_script` **stays**: `open_filtered` synthesizes a script through it for every scenery `.where()`. |
| 3.2 actions crate | **Done.** `RowVocab` is stateless: field access is an indexer (`row.email` routes to it), so predicates and `args:` share one static `Limits::Ui` host + cache instead of an engine per row. `ActionsMarker` + `actions_env`, `RowRefVocab`, `TablesVocab` + `tables_map`; `evaluate`/`evaluate_lifecycle` build a per-catalog host. `evaluate_predicate_with_controls` deleted (no callers). 73 tests. |
| 3.3 sites | **Done:** form submit, auth (`AuthVocab`), log filter (closure via `Compiled::engine()`/`ast()`), list text/label (**now `Template`** — YAML `text: '${ … }'`), view evaluator (`FormatVocab`+`ClockVocab`, scanner = `vantage_rhai::template::split`, per-hole rendering unchanged), projection + stat (shared `ui_scope::ui_host()`), wizard flow (3 sites; `step_env` in vantage-wizard), http body (`Expr` on a static host; `execute_http` lost its engine param), scripted narrowing. |
| 3.6 scenery direct reads | **Done in code.** `SceneryVocab`; `eval_in(script, env)` discovers reads through the page frame; `substitute_scope_reads` deleted. `ObservationResolver` now receives `&Arc<ui_scope::Scope>` instead of a text callback; `scope_reader(frame)` serves `substitute_key`. The shaped cache keys on script + read values. **Example YAML not yet migrated** (`"${ns.value}"` inside scenery strings must become `ns.value`). |
| 3.1 slot types | **Done.** `vantage-rhai` slots gained `Deref<Target = str>`, `AsRef<str>`, `Display`, `Default`, `PartialEq<str>` so a field can change kind without touching display-only readers; workspace dep enables the `schema` feature (`x-language: rhai` in every generated schema). Kinds: **`Expr`** — every `when:`/`condition:`, `RowAction.args.*`, `ProjectionDef.formula`, `FormFieldSpec.options`, `FormField.options`, `StreamSpec.filter`; **`Template`** — view-node text/value fields (`text`, `secondary`, `copy`, `label`, `value`, `unit`, `max`, `target`, `url`), `LabelParams.text`, `ProgressParams.*`, `ImageDef.src`, `ListParams.text/label`, `ViewMountDef.args.*`, `FinderParams.root`, `FormField.default`, `DialogSpec.{title,description,confirm_label,cancel_label}`, `StepSpec.title`; **`Block`** — `RowAction.action`, `ToolbarAction.action`, `ButtonParams.action`, `ToolbarSpec.action`, `WidgetDef.{on_ready,on_event}`, `FormDef.on_submit`, `ShapedObserveRef.script`, `SelectQueryBlock.rhai`, all driver-block scripts (`surreal.rhai/modify`, `sql.rhai`, `cmd.rhai/detail`, `faker.rhai`), `StepSpec.worker/workers.*`. Deliberately **left `String`** because their consumers never interpolate: select/chart/stat/csv/toolbar/filter labels, `ViewNode::List.empty`, `RecordViewDef.src`, `ViewMountDef.src`, `ParamDef.label`, `FormFieldSpec.default`, `FormDef.record` and `HttpSpec.url/headers` (scope-path / env templates, not Rhai). Runtime mirrors follow the slot (`Projection.formula`, `FormFieldRuntime`, formkit `FieldSpec`/`BlockSpec`, `ListFormat`); handoffs into the data layer's own specs convert with `to_string()`. |
| 3.4 remaining `${}` scanners | **Done.** `substitute_key`, `resolve_args_template*`, catalog + ui-mount validators, `rhai_import::apply_template` and the action `shape.rs` interpolators all scan with `vantage_rhai::template::split` and keep their own lookup. `grep 'find("${")'` over `crates framework` is empty. `substitute_env` / `extract_env_vars` stay (process env, not scope). |

vantage-rhai grew again: `Compiled::engine()` / `Compiled::ast()` accessors; `ui_scope::ui_host()`.

## Examples + docs sweep — DONE (uncommitted in both repos)

- vantage-ui-examples (`apps/cashgpt`, `periscope`, `space`): every scenery script reads
  scope directly (`month.value`, `appsel.selected_id`, `"models_top" + topn.value`) — 107
  lines across 10 pages, applied with one sed scoped to lines containing `scenery(`; the
  eight framework-list `text:`/`label:` params are `${ … }` templates; `# language=rhai`
  above every cashgpt table `rhai: |` block (68).
- vantage-ui `examples/surreal-bakery`: the import wizard's worker is
  `worker: !include import-products.rhai`; `# language=rhai` above the 16 inline bodies.
- Skills docs: `rhai-expressions.md` rewritten (three kinds, `${ }`, direct scope reads,
  `!include`, `# language=rhai`, bounded engines); `decorating-columns.md` loses the
  `render:`/`copy:`/`unit.rhai` section; `expressions:` gone from `SKILL.md` and
  `yaml-schemas.md`; `charts-and-dashboards.md` and `pages-and-components.md` show direct
  reads. Vendored `apps/*/.agents/skills` copies are untracked in the examples repo.
- Verification: `examples_validate` passes over the in-repo roots and, via
  `VANTAGE_EXTRA_INVENTORIES`, over cashgpt, periscope, space, launch-control,
  vantage-github and faker-demo. `ui-grammar` has a schema test asserting
  `x-language: rhai` on the three slot definitions and a `$ref` from `ButtonParams.action`.

## Release — PREPARED, waiting on commits

Done in the working trees (nothing committed: the 1Password SSH signer has been failing
all day — every `git commit` hangs on it):

- vantage-ui: no crate imports `rhai::` directly any more — 38 files now go through
  `vantage_rhai::rhai::` (actions, components, wizard, plugin-host) or `ui_scope::rhai::`
  (app, backend). The `rhai` dependency is gone from the four manifests that are mine;
  `crates/app/Cargo.toml` and `crates/backend/Cargo.toml` keep it because both carry the
  user's uncommitted chtags work (and `crates/backend/tests/chtags_import.rs` uses
  `rhai::Engine` directly), so the workspace `rhai` dep in the root `Cargo.toml` stays
  until that lands. Re-pinned: rhai 0.6.2, vista 0.6.25, sql 0.6.22, surrealdb 0.6.18,
  cmd 0.6.5, faker 0.6.15, diorama 0.12.5, api-client 0.6.13. App bumped to 0.38.0 via
  `script/bump-version` (touches `crates/app/Cargo.toml` — stage that hunk only — and
  `Info.plist`); `## Vantage 0.38` block written in `CHANGELOG.md`.
- vantage: versions bumped and dated changelog entries written for the eight crates above
  (sibling pins raised to the new versions).

Verification: vantage-ui `cargo check --workspace --all-targets` clean (two pre-existing
warnings), tests green in ui-grammar, inventory (+ `examples_validate` over every
external app root), catalog, actions, components, backend, wizard, plugin-host,
ui-component, ui-mount, ui-scope, and the app binary's own 116 tests. vantage-rhai 50
tests, clippy clean.

### Landing order (when the signer works again)

1. vantage-ui: commit the staged checkpoint (index intact; `cp /tmp/vui_worklock
   Cargo.lock` afterwards is no longer needed — the lock will be regenerated at step 4).
   Then stage the rest **by explicit path**, never `git add -A`: exclude `Cargo.lock`,
   `crates/backend/src/bundles.rs`, `crates/backend/tests/chtags_import.rs`, and stage only
   the version hunk of `crates/app/Cargo.toml` (`git diff -- crates/app/Cargo.toml` →
   `git apply --cached` the one hunk). Suggested message: "rhai unification: slot types,
   one template scanner, scenery direct reads, wizard and http hosts, examples and docs,
   0.38.0".
2. vantage-ui-examples: commit the sweep (cashgpt/periscope/space pages + tables).
3. vantage: commit `vantage-rhai` additions + bumps + changelogs + this file; merge; CD
   publishes in order (vantage-rhai first).
4. vantage-ui: once the crates are on crates.io, `cargo update -p vantage-rhai -p
   vantage-vista -p vantage-sql -p vantage-surrealdb -p vantage-cmd -p vantage-faker -p
   vantage-diorama -p vantage-api-client`, commit `Cargo.lock`, merge.

Not done: runtime checks against a rebuilt app (MCP `list_logs` on page open,
`preview_query` before/after direct reads, `perf_stats` on cashgpt `dashboard`, include
hot reload of `import-products.rhai`, faker tick in `app-select-tester`).
