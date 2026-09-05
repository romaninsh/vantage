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

## Phase 3 — vantage-ui sites, dead slots, scenery direct reads — NOT STARTED

## Sweep + release — NOT STARTED
