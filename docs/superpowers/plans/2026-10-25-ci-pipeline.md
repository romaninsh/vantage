# CI pipeline: compile once, test what changed — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace vantage's 13 PR workflows with one pipeline that compiles once, tests only changed crates, fails duplicate new tests, and runs little after merge; build it as layout A (PR 1) and layout C (PR 2), measure both, keep the faster.

**Architecture:** A `changes` job maps the diff to crates and drives every later job through outputs. A lint job checks the whole workspace in check mode. Tests run for changed crates only: layout A from an instrumented nextest archive fanned out to suite jobs, layout C in one instrumented `llvm-cov nextest` job. A Python checker reads the coverage matrix and fails new tests that land in overlap clusters.

**Tech Stack:** GitHub Actions (reusable workflows, composite actions, job outputs, concurrency), cargo-nextest, cargo-llvm-cov, Swatinem/rust-cache, octocov, Python 3 (stdlib + PyYAML, like `coverage/`).

**Spec:** `docs/superpowers/specs/2026-10-25-ci-pipeline-design.md`

## Global Constraints

- Every cargo command uses `--all-features` (excluded crates: the features their workflow uses today, unless `--all-features` builds).
- Jobs run on `ubuntu-latest`. Third-party actions pinned by commit SHA with a version comment.
- Rust caches: `save-if: ${{ github.ref == 'refs/heads/main' }}` (plus the temporary measurement condition, Task 8/10), one `shared-key` per profile: `ci-normal`, `ci-instrumented`, `ci-nightly`.
- Shared env on every Rust job: `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, `CARGO_PROFILE_TEST_DEBUG=line-tables-only`, `CARGO_TERM_COLOR=always`.
- `timeout-minutes` on every job; `concurrency: { group: ci-${{ github.event.pull_request.number || github.ref }}, cancel-in-progress: true }` on PR workflows.
- Do not touch `publish.yaml`, `publish-on-merge.yaml`, `pr-version-check.yaml`, or anything in the legacy `vantage/` crate.
- Never edit `coverage/exceptions/*.yaml` or `coverage/config.yaml`; exceptions come only from `coverage/vend-exception.sh`, run by the user.
- Commits: signed (`git commit -S`, check `%G?` = `G`), single-line lowercase imperative message. Commit once per PR at the end of its task group, not per task. Never push to `main`.
- Pause and report to the user at the end of Tasks 2, 9 and 11.
- Destructive or repo-settings actions (deleting caches, setting secrets, changing rulesets) are the user's: propose the exact command, don't run it.
- Workflow files must pass `actionlint` (installed at `/opt/homebrew/bin/actionlint`) before pushing.

## Review Focus

1. **A PR that changes only an excluded crate (e.g. `vantage-aws/src/…`)** must run that crate's suite, not nothing. Pinned in Task 3's `excluded_crate_change_selects_its_suite`.
2. **A version-bump-only PR** (crate `Cargo.toml` + `CHANGELOG.md` + workspace-member lines in `Cargo.lock`) must test exactly that crate, not everything. Pinned in Task 3's `lockfile_member_bump_is_not_global`.
3. **A test whose location is `?:0-0`** (mock contract tests) must never count as new or crash the checker. Pinned in Task 1's `unknown_location_is_never_new`.
4. **Every job skipped (docs-only PR)** must leave `ci-ok` green; any job failed or cancelled must make it red. Pinned in Task 5's `ci-ok` step and verified in Task 8.
5. **A `labeled` event for a label other than `ALL_TESTS`** must not start a second full run. Pinned in Task 5's workflow `if:` and verified in Task 8.

---

## Phase 0 — duplicate-test checker, validated on history

### Task 1: `coverage/new_tests.py`

**Files:**
- Create: `coverage/new_tests.py`
- Create: `coverage/test_new_tests.py` (stdlib `unittest`)
- Modify: `coverage/exceptions.py:32-40` (`key()`)
- Modify: `coverage/INSTRUCTIONS.md` (one short section: "In CI")

**Interfaces:**
- Consumes: `overlap.load(out, crate, package, jaccard) -> ns` with `ns.clusters: list[list[key]]` (anchor first), `ns.loc[key] = (file_rel_to_crate, first, last, _)`, `ns.sets`, `ns.exempt: dict[key, entry]`, `ns.rejected: list[(entry, problem)]`, `ns.short(key)`, `ns.where(key)`.
- Produces:
  - `changed_lines(base: str, paths: list[str]) -> dict[str, set[int]]` — repo-relative path → head line numbers added or modified (`git diff -U0 --no-color <base>...HEAD`).
  - `new_tests(ns, crate_dir_rel: str, changed: dict[str, set[int]]) -> set[key]` — tests whose `loc` range intersects changed lines; location `?` never new.
  - `violations(ns, new: set[key]) -> list[Violation]` with `Violation(kind: str, test: str, cluster: list[str], anchor: str)`, kinds `"joins-existing"`, `"new-with-new"`, `"rejected-exception"`.
  - `new_internal(base: str, crate_dirs: list[str]) -> list[str]` — `file:line` of added `#[cfg(test)]` lines under `<crate>/src/`, plus added files matching `<crate>/src/**/tests*`.
  - CLI: `python3 -I coverage/new_tests.py clusters <package> <crate_dir> <base>` (reads `coverage/report/<package>/` produced by the matrix; exit 1 on violations) and `python3 -I coverage/new_tests.py internal <base> <crate_dir>...` (exit 1 on any).
  - `exceptions.key()` returns `os.environ["COVERAGE_EXCEPTIONS_KEY"]` when set, else the file as today.

- [ ] **Step 1: Write the failing tests** in `coverage/test_new_tests.py`, building a fake `ns` with `types.SimpleNamespace` (no cargo, no matrix run):
  - `test_new_test_joining_old_cluster_fails`: cluster `[old_a, new_b]`, `new_b` lines changed → one `joins-existing` violation naming `new_b`, anchor `old_a`.
  - `test_new_anchor_over_old_test_fails`: cluster `[new_a, old_b]`, `new_a` changed → `joins-existing`.
  - `test_two_new_tests_in_one_cluster_fail`: cluster `[new_a, new_b]` → `new-with-new` once per cluster.
  - `test_old_only_cluster_passes`: cluster `[old_a, old_b]`, nothing changed → `[]`.
  - `test_exempt_new_test_passes`: `new_b` in `ns.exempt` → `[]`.
  - `test_unknown_location_is_never_new`: `loc[k] = ("?", 0, 0, None)` → not in `new_tests` even when changed lines exist.
  - `test_edit_inside_range_counts_as_new`: changed line 30, test range 24–36 → new; changed line 37 → not new.
  - `test_rejected_exception_fails`: `ns.rejected = [({"test": "x"}, "signature doesn't match")]` → `rejected-exception`.
  - `test_key_prefers_env`: with `COVERAGE_EXCEPTIONS_KEY=abc`, `exceptions.key() == "abc"`.
  - `test_changed_lines_parses_hunks`: feed a canned `git diff -U0` text to the hunk parser (factor `parse_hunks(text) -> dict[str, set[int]]` out of `changed_lines`): `@@ -10,0 +11,3 @@` → `{11,12,13}`; `@@ -5 +5 @@` → `{5}`; deletion-only `@@ -7,2 +6,0 @@` → `{}`.
  - `test_new_internal_finds_cfg_test_and_tests_files`: canned diff adding `#[cfg(test)]` in `vantage-x/src/lib.rs` and a new file `vantage-x/src/foo/tests.rs` → both reported; a `#[cfg(test)]` added under `vantage-x/tests/` → not reported.

- [ ] **Step 2: Run, expect failures**

  Run: `python3 -I -m unittest coverage/test_new_tests.py -v` (from repo root; the test adds `coverage/` to `sys.path`)
  Expected: errors — `new_tests` module not found.

- [ ] **Step 3: Implement** `coverage/new_tests.py` and the `key()` change. Failure output, one block per violation:

  ```
  vantage-vista: new test `vista_api::id_generator::short_ids_reach_every_character_of_a_small_alphabet`
    joins cluster 23 (anchor `rhai_vocab::...::retried_new_record_flash_reuses_its_generated_id`, 13 tests)
    delete the test it duplicates, or, if it asserts a distinct outcome, ask a human to run:
    coverage/vend-exception.sh vantage-vista vista_api::id_generator::short_ids_reach_every_character_of_a_small_alphabet "<reason>"
  ```

  Cluster numbers follow `ns.clusters` order (1-based, same as `clusters-NN.md`). Also write the same text to `$GITHUB_STEP_SUMMARY` when that env var is set.

- [ ] **Step 4: Run, expect pass** — same command; all 11 pass.

- [ ] **Step 5: INSTRUCTIONS.md** — add a 4–6 line "In CI" section: what fails (the three kinds plus new internal tests), that only new or edited tests are judged, and that the only way out is a vended exception. No commit yet (Phase 0 lands in PR 1).

### Task 2: Validate the checker on five merged PRs

**Files:** none committed. Write the findings to `.superpowers/sdd/ci-pipeline/validation.md` (git-ignored).

- [ ] **Step 1:** For each of #472 (`ca159cb4`), #471 (`a128a142`), #466 (`ea5cfe11`), #465 (`f4386313`), #464 (`4ac8b5b9`): in a detached checkout of the merge commit, base = `<merge>^1`, list changed crates with `git diff --name-only <merge>^1 <merge>`. Use `git worktree` only if the user authorizes it; otherwise check out the commit in the main tree after confirming `git status` is clean, and return to the original branch afterwards.
- [ ] **Step 2:** Start databases the changed crates need with their `scripts/ingress.sh` (never raw docker/psql), then per changed crate with tests run `coverage/matrix.sh <crate>` and `python3 -I coverage/new_tests.py clusters <crate> <dir> <merge>^1`, plus `python3 -I coverage/new_tests.py internal <merge>^1 <dirs>`.
- [ ] **Step 3:** Record per PR: crates, matrix minutes, violations (test, cluster size, anchor), and a judgement per violation: real duplicate / distinct outcome (exception-worthy) / noise. Expected for #472: vista cluster 23 members from `tests/vista_api/id_generator.rs`, and diorama cluster 14's `concurrent_flashes_share_one_late_minted_id`.
- [ ] **Step 4: Pause.** Show the user the table and the noise rate. If noise is high, propose a rule or `jaccard` change and wait for a decision before Phase 1.

---

## Phase 1 — PR 1: shared pipeline + layout A (branch `ci-build-once` from `main`)

### Task 3: `script/ci-changes`

**Files:**
- Create: `script/ci-changes` (Python 3, executable, `#!/usr/bin/env python3`)
- Create: `script/test_ci_changes.py` (`unittest`)
- Create: `.github/ci-crates.yaml`

**Interfaces:**
- `.github/ci-crates.yaml` is the single mapping:
  ```yaml
  services:            # crate -> databases its tests need
    vantage-sql: [postgres, mysql]
    vantage-surrealdb: [surreal]
    surreal-client: [surreal]
    bakery_model3: [surreal, postgres]   # verify against coverage.yaml env
    vantage-mongodb: [mongo]
  global:              # any change here = every crate changed
    - Cargo.toml
    - rust-toolchain*
    - .github/**
    - .config/**
    - coverage/**
  not_global:
    - coverage/exceptions/**
  book: [docs4/**]
  excluded:            # crates outside the workspace, each with its own suite
    vantage-aws:        { gated: true }
    vantage-kubernetes: { gated: true }
    vantage-cmd:        {}
    vantage-spacetimedb: {}
    vantage-ui-adapters: {}
    example_*:          { gated: true }   # ui-examples
  ```
  Fill `services` from `coverage.yaml`, `surreal.yaml`, `mongodb.yaml`, `vantage-sql.yaml` (including `docs4` if its doctests touch a database).
- Produces on stdout (and `$GITHUB_OUTPUT` as `key=json` lines when set): `crates` (workspace members, sorted JSON list), `excluded` (excluded crates to run), `services` (sorted union), `all` (bool), `book` (bool), `gated` (bool: `ALL_TESTS` label present).
- CLI: `script/ci-changes --base <sha> --head <sha> [--labels <json list>]`; workspace members from `cargo metadata --no-deps --format-version 1`; excluded crate dirs from root `Cargo.toml` `exclude` globs expanded on disk.
- Pure core for tests: `classify(files: list[str], lock_diff: str, members: dict[str,str], excluded: dict[str,str], config: dict, labels: list[str]) -> dict`.

- [ ] **Step 1: Failing tests** in `script/test_ci_changes.py` against `classify`:
  - `crate_file_selects_its_crate`: `vantage-vista/src/lib.rs` → `crates == ["vantage-vista"]`, `all` false.
  - `version_bump_selects_its_crate`: `vantage-vista/Cargo.toml`, `vantage-vista/CHANGELOG.md` → `["vantage-vista"]`.
  - `lockfile_member_bump_is_not_global`: lock diff changing only `version = ` under `name = "vantage-vista"` → `all` false.
  - `lockfile_external_change_is_global`: lock diff changing `name = "serde"`'s version → `all` true, `crates` = every member.
  - `excluded_crate_change_selects_its_suite`: `vantage-aws/src/lib.rs` → `excluded == ["vantage-aws"]`, `crates == []`.
  - `workflow_change_is_global` (`.github/workflows/ci.yaml`) and `exceptions_change_is_not_global` (`coverage/exceptions/vantage-vista.yaml` → `crates == []`, `all` false).
  - `docs_change_turns_on_book_only`: `docs4/src/intro.md` → `book` true and `crates == ["docs4"]`.
  - `all_tests_label_runs_everything`: labels `["ALL_TESTS"]` → `all` true, `gated` true, `excluded` = every excluded crate.
  - `services_follow_crates`: `vantage-sql/src/x.rs` → `services == ["mysql", "postgres"]`.
  - `longest_prefix_wins`: with members `vantage-api-client` and `vantage-api-client-x`, `vantage-api-client-x/src/a.rs` → only the latter.
- [ ] **Step 2:** `python3 -I -m unittest script/test_ci_changes.py -v` → fails (no module).
- [ ] **Step 3:** Implement. Keep `classify` free of I/O; `main()` gathers files (`git diff --name-only base...head`), the lock diff (`git diff base...head -- Cargo.lock`), metadata and config.
- [ ] **Step 4:** Same command → all pass. Then run `script/ci-changes --base origin/main~1 --head origin/main` locally and check the output is sane for the last merge.

### Task 4: Composite setup action

**Files:** Create `.github/actions/setup-rust/action.yml`

**Interfaces:** inputs `profile` (`normal` | `instrumented` | `nightly`), `tools` (comma list for `taiki-e/install-action`, may be empty), `save` (string `"true"`/`"false"`, default `${{ github.ref == 'refs/heads/main' }}` computed by the caller). Steps: toolchain (`dtolnay/rust-toolchain`, stable or the pinned nightly from `public-api.yaml`), `Swatinem/rust-cache` with `shared-key: ci-${{ inputs.profile }}`, `save-if: ${{ inputs.save }}`, `cache-all-crates: true`, `cache-on-failure: true`; `install-action` when `tools` non-empty; exports the shared env from Global Constraints to `$GITHUB_ENV`. Checkout stays in each job.

- [ ] **Step 1:** Write it. **Step 2:** `actionlint` (it checks composite actions referenced from workflows once Task 5 exists; re-run there).

### Task 5: `ci.yaml` — changes, lint, excluded suites, gated suites, `ci-ok`

**Files:**
- Create: `.github/workflows/ci.yaml`
- Create: `.github/workflows/suite-excluded.yaml` (`on: workflow_call`, input `crate`)
- Delete: `main.yaml`, `aws.yaml`, `cmd.yaml`, `kubernetes.yaml`, `spacetimedb.yaml`, `ui-adapters.yaml`, `ui-examples.yaml`, `repo-layers.yaml`, `public-api.yaml` (folded into lint)
- Keep for now (replaced in Task 6): `coverage.yaml`, `surreal.yaml`, `mongodb.yaml`, `vantage-sql.yaml`

**Interfaces:**
- Trigger: `pull_request` on `[main, v2]`, `types: [opened, synchronize, reopened, labeled]`. Every job after `changes` carries `if: github.event.action != 'labeled' || github.event.label.name == 'ALL_TESTS'`.
- `changes` outputs: exactly Task 3's keys.
- `lint` (`profile: normal`): fmt; clippy whole workspace `--all-targets --all-features -D warnings`; `cargo doc --no-deps --all-features -p <each changed crate>` with `RUSTDOCFLAGS=-D warnings`; `script/check-repo-layers`; `python3 -I coverage/new_tests.py internal <base> <changed crate dirs>`; public-api for changed crates that have `api/<crate>.txt` (separate step with `profile: nightly` toolchain — make it its own job `public-api` if toolchains can't share a job cleanly).
- `excluded` job: matrix `crate: ${{ fromJSON(needs.changes.outputs.excluded) }}`, `uses: ./.github/workflows/suite-excluded.yaml`, `secrets: inherit` (AWS). The reusable workflow holds the per-crate steps lifted from the deleted workflows (minikube + helm for kubernetes, AWS CLI smoke, ui-examples per example), each in `working-directory: <crate>` with its own cache key `ci-excluded-<crate>`.
- `ci-ok`: `needs:` every job, `if: always()`, one step: fail when `contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled')`.

- [ ] **Step 1:** Investigate `script/check-repo-layers` (88s): note in the ledger what it spends time on; if it builds or resolves metadata repeatedly, make it reuse the lint job's build or run after clippy. Don't change what it checks.
- [ ] **Step 2:** Write `ci.yaml` and `suite-excluded.yaml`; delete the listed workflows.
- [ ] **Step 3:** `actionlint` → clean.

### Task 6: Layout A test jobs

**Files:**
- Modify: `.github/workflows/ci.yaml`
- Delete: `coverage.yaml`, `surreal.yaml`, `mongodb.yaml`, `vantage-sql.yaml`
- Modify: `coverage/matrix.sh` — add `MATRIX_ARCHIVE=<file>`: when set, skip building, list and run from the archive (`--archive-file`, `--workspace-remap .`) instead of `cargo nextest list -p`.
- Modify: `.octocov.yml` — per-crate comparison (see Step 4).

**Interfaces:**
- `build` (needs `changes`, `if: crates != []`, `profile: instrumented`, tools `cargo-llvm-cov,cargo-nextest`): `cargo llvm-cov nextest-archive --all-features -p <crate>... --archive-file ci.tar.zst` (if `nextest-archive` doesn't exist in the pinned cargo-llvm-cov, use `eval "$(cargo llvm-cov show-env --export-prefix)"` + `cargo nextest archive` and note it). Upload with `actions/upload-artifact` (`retention-days: 1`, `compression-level: 0`).
- `suite` (needs `build`): matrix over service groups computed in `changes` (add output `groups`: list of `{name, crates, services}`; crates with no database go to group `plain`). Each job starts only its services (`services:` with `if`-free container defs per group via `include` entries), downloads the archive, runs `cargo nextest run --archive-file ci.tar.zst --workspace-remap . -E '<package() filterset>'` under `cargo llvm-cov` env, uploads its `*.profraw` as `profraw-<group>`.
- `doctest-bdd` (needs `changes`, `profile: normal`): `cargo test --doc --all-features -p <crates>`; `cargo test -p vantage-diorama --all-features --test bdd` when diorama changed.
- `coverage` (needs `suite`): download archive + all `profraw-*`, `cargo llvm-cov report --lcov --output-path lcov.info` with the existing `IGNORE` regex from the deleted `coverage.yaml`, run octocov.
- `duplicates` (needs `build`, services as needed by changed crates): download archive, `MATRIX_ARCHIVE=ci.tar.zst coverage/matrix.sh <crate>` then `coverage/new_tests.py clusters` per changed crate; env `COVERAGE_EXCEPTIONS_KEY: ${{ secrets.COVERAGE_EXCEPTIONS_KEY }}`.
- Gated `vantage-sql` MariaDB and `bakery_model3` CLI smokes: one job `gated-sql`, `if: needs.changes.outputs.gated == 'true'`, steps lifted from `vantage-sql.yaml`.

- [ ] **Step 1:** Add the `groups` output to `script/ci-changes` with test `groups_split_by_service`: `vantage-sql` + `vantage-vista` changed → groups `[{name: "sql", crates: ["vantage-sql"], services: ["mysql","postgres"]}, {name: "plain", crates: ["vantage-vista"], services: []}]`. Run the unittest file → pass.
- [ ] **Step 2:** `MATRIX_ARCHIVE` in `matrix.sh`. Verify locally: `cargo llvm-cov nextest-archive -p vantage-vista --all-features --archive-file /tmp/v.tar.zst` then `MATRIX_ARCHIVE=/tmp/v.tar.zst coverage/matrix.sh vantage-vista` produces the same `clusters-*.md` row set as a plain run.
- [ ] **Step 3:** Write the jobs; delete the four workflows.
- [ ] **Step 4:** octocov per crate: one octocov run per changed crate with `repository: ${{ github.repository }}/<crate>` (octocov's monorepo key) over that crate's lcov slice, so PR and main compare like for like. If octocov can't key that way, report it and propose an alternative instead of shipping a whole-workspace number.
- [ ] **Step 5:** `actionlint` → clean.

### Task 7: Post-merge and Sonar

**Files:**
- Create: `.github/workflows/post-merge.yaml` (`push` to `main`)
- Modify: `.github/workflows/sonar-build.yml` (`pull_request` + `schedule: weekly` on main; drop `push`)
- Modify: `.github/workflows/book.yaml` only if its `push` trigger lacks the `docs4/**` path filter

**Interfaces:** `post-merge.yaml` jobs: `changes` (base `github.event.before`), `coverage` for changed crates (`profile: instrumented`, `save: true`, uploads octocov report per crate), `lint-cache` (`cargo clippy --workspace --all-targets --all-features`, `profile: normal`, `save: true`, no `-D warnings` gate needed). No tests beyond coverage's run, no Sonar, no public-api.

- [ ] **Step 1:** Write it. **Step 2:** `actionlint` → clean.

### Task 8: Open PR 1 and verify behaviour

- [ ] **Step 1:** Add the temporary measurement condition to `setup-rust` callers: `save: ${{ github.ref == 'refs/heads/main' || github.head_ref == 'ci-build-once' || github.head_ref == 'ci-measure' }}`, with a `# TEMP: measurement` comment.
- [ ] **Step 2:** Run all Python tests (`coverage/test_new_tests.py`, `script/test_ci_changes.py`), `actionlint`. Commit signed (single line), push `ci-build-once`, open PR 1 as draft against `main` with the spec and plan files included.
- [ ] **Step 3:** Ask the user to set the secret (don't run it yourself): `gh secret set COVERAGE_EXCEPTIONS_KEY --repo romaninsh/vantage < ~/.config/vantage/coverage-exceptions.key`.
- [ ] **Step 4:** Behaviour checks on throwaway branches stacked on `ci-build-once` (draft PRs, closed after): docs-only change → only `changes`, book, `ci-ok` green; `vantage-aws` comment change → excluded suite runs; adding label `DOCS` → no rerun; adding `ALL_TESTS` → gated suites run; a branch adding a test copied from an existing vista test → `duplicates` fails with the vend command.

### Task 9: Measure A

- [ ] **Step 1:** Measurement branch `ci-measure` from `ci-build-once` with a one-line comment change in `vantage-vista/src/lib.rs`. Push once to seed caches (doesn't count), then three runs (empty commits), then one cold run with `shared-key` prefix bumped in a commit. Repeat the three warm runs with `ALL_TESTS`.
- [ ] **Step 2:** For each run collect via `gh api repos/romaninsh/vantage/actions/runs/<id>/jobs`: wall time first job start → `ci-ok` end, summed job durations (runner-minutes), per-job durations. Write the table to `.superpowers/sdd/ci-pipeline/measure-a.md`.
- [ ] **Step 3: Pause.** Report A's numbers next to today's baseline (6.5 min wall, 31 runner-min). Wait for "continue".

---

## Phase 2 — PR 2: layout C (branch `ci-instrumented-run` from `ci-build-once`)

### Task 10: Replace A's test jobs with `test-cov`

**Files:** Modify `.github/workflows/ci.yaml`, `.github/workflows/post-merge.yaml` only where the coverage job shape changes.

**Interfaces:** remove `build`, `suite`, `coverage`, `duplicates`; add `test-cov` (needs `changes`, `profile: instrumented`, services: the union from `changes.outputs.services` — use a matrix-of-one with `include` per service combination or start containers with `docker run` steps gated by `contains(fromJSON(needs.changes.outputs.services), 'postgres')`, whichever actionlint accepts and keeps the job single): `cargo llvm-cov nextest --all-features -p <crates> --no-report`, then `coverage/matrix.sh` + `new_tests.py clusters` per crate reusing the same target dir (no `MATRIX_ARCHIVE`), then `cargo llvm-cov report --lcov` and octocov per crate. `doctest-bdd`, lint, excluded, gated, `ci-ok` unchanged. Update `ci-ok`'s `needs`.

- [ ] **Step 1:** Edit; `actionlint` clean. Keep the TEMP save condition, adding `ci-instrumented-run` and `ci-measure-c`.
- [ ] **Step 2:** Commit signed, push, open PR 2 as draft against `ci-build-once`.

### Task 11: Measure C and decide

- [ ] **Step 1:** Same protocol as Task 9 on `ci-measure-c` from `ci-instrumented-run`. Table to `.superpowers/sdd/ci-pipeline/measure-c.md`.
- [ ] **Step 2: Pause.** Show both tables side by side. Rule from the spec: lower median warm wall time wins; within 10%, fewer runner-minutes wins. Recommend; the user decides which PR to close.
- [ ] **Step 3:** After the decision: remove the `# TEMP: measurement` save conditions from the winner, close the throwaway measurement PRs and the losing PR, and propose (don't run) the cache cleanup command for old keys and the ruleset change making `ci-ok` required.
