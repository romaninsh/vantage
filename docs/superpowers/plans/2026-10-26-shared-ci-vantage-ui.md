# Shared CI pipeline, first consumer vantage-ui: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make vantage's CI pipeline callable from another repo and move atk4/vantage-ui's PR CI
onto it.

**Architecture:**
- romaninsh/vantage turns `changes`, `tests`, `audit` and `summary` into reusable workflows.
  Every shared job checks out vantage itself at `job.workflow_sha` into `.ci/` and runs scripts
  from there against the caller's workspace.
- Each consumer has a thin `ci.yaml`, its own `.github/ci-crates.yaml` and an optional
  `.github/ci-setup` hook.
- vantage-ui references a vantage branch during development and switches to a commit sha before
  it merges.

**Tech Stack:** GitHub Actions (reusable workflows, composite actions), Python 3 (`script/ci-changes`,
`coverage/*.py`), bash, cargo-nextest, cargo-llvm-cov, WarpBuild runners and cache.

**Spec:** `docs/superpowers/specs/2026-10-26-shared-ci-vantage-ui-design.md`

## Global Constraints

- **Branches and PRs.** vantage work happens on branch `shared-ci` with a draft PR in
  romaninsh/vantage. vantage-ui work happens on branch `shared-ci` with a draft PR in
  atk4/vantage-ui.
  - During development, vantage-ui references `romaninsh/vantage/...@shared-ci`.
  - Before vantage-ui merges, it references the vantage merge commit's sha.
- **Verification (house rule: no stub unit tests for CI scripts).** CI scripts are verified by:
  - replaying them on real commits;
  - `SHELLCHECK_OPTS=--severity=warning actionlint`;
  - `python3 -m py_compile`;
  - real GitHub runs.

  Every "test" step below is one of those.
- **Edits and commits.**
  - Edit files with the Edit and Write tools, never sed or heredocs.
  - Commits are signed (`git commit -S`, then check that `git log -1 --format=%G?` prints `G`).
  - Commit messages are single-line, lowercase, with no AI attribution.
  - Commit and push only at the commit steps below.
- **Secrets.**
  - Shared workflows declare `CI_SSH_KEYS` and `COVERAGE_EXCEPTIONS_KEY` under
    `on.workflow_call.secrets`, both `required: false`.
  - Callers pass them by name. Never use `secrets: inherit` across accounts.
  - `COVERAGE_EXCEPTIONS_KEY` reaches only the `new_tests.py` and `exceptions.py` steps, never
    PR-built code.
- **Leave alone.** No edits to vantage-ui's nightly, draft-release, release, plugin-publish or
  book workflows. In vantage, don't touch the legacy `vantage/` crate or
  `publish-on-merge.yaml`.
- **Anti-goal: vantage gets slower.** With defaults, vantage's job count and its `changes`
  outputs for the replay commits (ca159cb4, ea5cfe11, 4ac8b5b9, d2ba4f4b) must not change. The
  new keys are additive.
- **Phase pauses.** Stop at the end of each phase, report, and wait for the user's "continue".
- **User actions.** The user creates repo secrets and changes repo settings. Propose these;
  don't do them.

## Prerequisites (user / other PRs, before Phase B's trial)

1. vantage `ci-all-tests-fixes` is merged: version check on touched crates only, and the apt hang
   fix.
2. vantage-ui PR #283 is merged: nextest, `.config/nextest.toml`, `cargo test --doc`.
3. The vantage-ui acp flake `exit_during_turn_ends_turn_and_reports_stderr` is fixed or
   quarantined.
4. Secrets exist in atk4/vantage-ui: `CI_SSH_KEYS` (both deploy keys, newline-joined) and
   `COVERAGE_EXCEPTIONS_KEY`.

## Review Focus

1. **A caller whose `ci-crates.yaml` lacks every new key** (vantage itself). Expect outputs
   byte-identical to today's for the replay commits. This is pinned in Task 2, step 2.
2. **An app-mode PR that only bumps the app version** (Cargo.toml version line, Cargo.lock entry,
   Info.plist, CHANGELOG). Expect `crates=[]` and `app.owed=false`, with `app.new` set. Pinned in
   Task 2, step 6.
3. **A dependency-only vantage-ui PR** (Cargo.lock / vantage-lib rev bump). Expect `all=true` and
   `app.owed=true`. Pinned in Task 2, step 6.
4. **A run where `CI_SSH_KEYS` is unset** (vantage, forks). Expect the ssh-agent step to skip,
   not fail. Pinned in Task 4, step 4.
5. **More than 6 groups.** Expect `ci-changes` to exit 1 with
   `::error::7 test groups; tests.yaml has 6 archive slots`. Pinned in Task 2, step 4.

---

## Phase A: shared pipeline in romaninsh/vantage (branch `shared-ci`)

### Task 1: Scripts find the workspace from the current directory

**Files:**
- Modify: `script/ci-changes` (`ROOT`, line 48)
- Modify: `coverage/matrix.sh` (`root=`, line 34)
- Modify: `coverage/new_tests.py` (`ROOT`, line 27)
- Modify: `coverage/exceptions.py` (`DIR`, line 23, and the `cwd` at line 67)
- Modify: `coverage/config.py` (`FILE`, line 7)

**Interfaces:**
- Produces:
  - Every script's root is `git rev-parse --show-toplevel` run in the current directory.
  - `coverage/config.py` `FILE` is `<root>/coverage/config.yaml` if it exists, else the copy
    next to the script.
  - `exceptions.DIR` is `<root>/coverage/exceptions`. A missing directory means no exceptions,
    and is not an error.
  - Script-relative paths stay for the scripts' own siblings: `matrix-rs`, `groups.py`, and the
    imports of `config` and `exceptions`.

- [ ] **Step 1: Create the branch and draft PR.** Branch `shared-ci` from `origin/main`. Push an
  empty signed commit `start the shared ci pipeline`. Open the PR with `gh pr create --draft
  --title "shared ci pipeline for other repos"` and a two-line body that links the spec.
- [ ] **Step 2: Capture today's outputs.** In `/Users/rw/Work/vantage`, run `script/ci-changes
  --base X~1 --head X` for X in ca159cb4, ea5cfe11, 4ac8b5b9 and d2ba4f4b, and once with
  `--base 08f3a3b9~1 --head 08f3a3b9 --labels '["ALL_TESTS"]'`. Save each to
  `/tmp/ci-before/<X>.txt`.
- [ ] **Step 3: Implement the root changes** listed under Interfaces.
- [ ] **Step 4: Replay from another directory.**
  - Run `cd /Users/rw/Work/vantage && /tmp/shared-ci/script/ci-changes ...`, where
    `/tmp/shared-ci` is `git worktree add /tmp/shared-ci shared-ci`, so the script runs from
    outside the workspace it reads.
  - Expected: `diff` against `/tmp/ci-before/*` is empty for all five.
  - Then `cd /Users/rw/Work/vantage && /tmp/shared-ci/coverage/matrix.sh --baseline
    vantage-types` writes into `/Users/rw/Work/vantage/coverage/report/vantage-types/`.
- [ ] **Step 5: Commit** `scripts take the workspace from the current directory, so another repo
  can run them from .ci`. Then remove `/tmp/shared-ci`.

### Task 2: `ci-changes` learns the consumer keys

**Files:**
- Modify: `script/ci-changes`
- Modify: `.github/ci-crates.yaml` (comments documenting the new keys; vantage sets none of them)

**Interfaces:**
- Consumes: Task 1's root.
- Produces these new output keys, always printed. Defaults are given for a config without the
  key.

  | Output | Value | Default |
  |---|---|---|
  | `runner` | JSON string | `"ubuntu-latest"` |
  | `build_runner` | JSON string | `runner` |
  | `small_runner` | JSON string | `"ubuntu-latest"` |
  | `env` | JSON object | `{}` |
  | `setup` | `true` if `.github/ci-setup/action.yml` exists | — |
  | `warm` | `"changed"` or `"full-on-miss"` | `"changed"` |
  | `archive_exclude` | nextest filter string | `""` |
  | `skill_url` | string | vantage's current SKILL URL |
  | `versioning_mode` | `"crates"`, `"app"` or `"none"` | `"crates"` |
  | `app` | JSON object (app mode), see below | `{}` |

  - `app` in app mode is `{"crate", "old", "new", "owed": bool, "changelog": {"path", "text"} |
    null, "plist": "<CFBundleShortVersionString>" | null, "plist_ok": bool, "skip_label"}`.
  - Each `groups` entry gains `"slot": 1..6`, in sorted group order.
- New behaviour:
  - `dependents: true` expands `crates`, `tested`, `libs`, `measured` and `groups` to the reverse
    transitive closure over workspace members. `cargo metadata --no-deps`: each package's
    `dependencies` with a `path`, all kinds.
  - In app mode, files that are only the version bump don't select the app crate:
    - the app crate's Cargo.toml when the only changed line is `version = `;
    - its Cargo.lock entry;
    - the plist;
    - the root CHANGELOG.
  - `app.owed` is true when the diff touches any workspace crate directory, the root
    `Cargo.toml`, or `Cargo.lock`.
  - `app.changelog` is the `## Vantage {major}.{minor}` block from `versioning.changelog`, made
    one line by the existing `changelog_entry` rules, using the heading template from
    `versioning.heading`.
  - `plist_ok` means both `CFBundleShortVersionString` and `CFBundleVersion` equal `new`.
  - More than 6 groups: print `::error::<n> test groups; tests.yaml has 6 archive slots` and
    exit 1.

- [ ] **Step 1: Write the draft vantage-ui config** at `/tmp/ui-ci-crates.yaml`. Use the spec's
  example block with `runner: warp-ubuntu-latest-arm64-8x`, `dependents: true`, `warm:
  full-on-miss`, `archive_exclude: "binary(compile_fail)"`, `versioning.mode: app`, `crate:
  vantage-ui`, `plist: crates/app/vantage-ui/resources/Info.plist` and `skip_label:
  skip-changelog`. Add `global: [Cargo.toml, rust-toolchain*, .config/**]`, `ci_self:
  [".github/**"]` and `coverage_ignore: ["vantage-ui", "catalog", "auto-update", "feeds-ui", "services-ui",
  "agent-ui", "csv-dio", "form-tester", "inventory-viewer", "select-tester", "sqlite-dio",
  "story"]`. Take the exact package names from `cargo metadata` in vantage-ui.
- [ ] **Step 2: Implement the defaults and the plain outputs** (runner, env, setup, warm,
  archive_exclude, skill_url, versioning_mode, and slot on groups). Replay Task 1's five commits
  in vantage.
  - Expected: `diff <(grep -v -E '^(runner|build_runner|small_runner|env|setup|warm|archive_exclude|skill_url|versioning_mode|app)=' new) /tmp/ci-before/X.txt`
    shows only the added `"slot":n` in `groups`.
- [ ] **Step 3: Implement `dependents`.** In a scratch copy of vantage-ui (`git -C
  /Users/rw/Work/vantage-ui worktree add /tmp/ui-replay gh/main`), with `/tmp/ui-crates.yaml`
  copied to `.github/ci-crates.yaml` there, replay on a recent vantage-ui commit that touched
  only `crates/framework/core`. Find it with `git log --format=%h -- crates/framework/core`.
  - Expected: `crates` holds framework/core plus every member whose dependency closure reaches
    it. Cross-check against `cargo tree -i <core package> --workspace -e normal,dev,build
    --prefix none` (deploy keys come from the local `.cargo` patches; see note).
  - Expected: on a commit touching one leaf widget crate, `crates` holds only that crate.
- [ ] **Step 4: Implement the 6-slot cap.** Replay vantage with a temporary config that maps
  seven crates to seven distinct fake services.
  - Expected: exit 1, plus the exact error line from Interfaces.
- [ ] **Step 5: Implement app mode** (outputs `app`, the app-bump exclusion and `owed`).
- [ ] **Step 6: Replay app mode** in `/tmp/ui-replay` on three real vantage-ui commits:
  - a release-bump-only commit (`git log --format='%h %s' -- crates/app/vantage-ui/Cargo.toml`
    and pick one that changed only the version, plist and CHANGELOG). Expected: `crates=[]`,
    `app.owed=false`, `app.new` set;
  - a feature commit in one crate that also bumped the version. Expected: `crates` contains that
    crate and its dependents but not `vantage-ui`, unless it depends on it. Also `app.owed=true`,
    `app.changelog` not null and `plist_ok=true`;
  - a Cargo.lock-only commit. Expected: `all=true`, `app.owed=true`.
- [ ] **Step 7: Lint and commit.** `python3 -m py_compile script/ci-changes` (via a copy with a
  `.py` name), then commit `ci-changes reads runner, env, dependents and app versioning from the
  consumer's ci-crates.yaml`. Remove `/tmp/ui-replay`.

Note: in vantage-ui, `cargo metadata --no-deps` needs no network. `cargo tree` does, so use it
only for the step 3 cross-check, run in `/Users/rw/Work/vantage-ui`, where the local `.cargo`
patches resolve the private dependencies.

### Task 3: Summary supports app mode and the skill URL

**Files:**
- Modify: `coverage/ci-report.py`

**Interfaces:**
- Consumes: Task 2's outputs inside `--run`: `versioning_mode`, `app`, `skill_url`.
- Produces:
  - In app mode the table has no `version` column. Under the headline there is one line:
    - `vantage-ui [1.9.9](<changelog url> "<entry>") (+patch)`; or
    - `💤 vantage-ui not bumped`; or
    - `📝 no CHANGELOG block for Vantage 1.9`; or
    - `🍎 Info.plist says 1.9.8`.

    With `app.owed=false` and no bump, the line is omitted.
  - A new argument, `--labels <json list>`. When `app.skip_label` is in it, the app line gets
    ` · skipped by \`skip-changelog\``.
  - `SKILL` comes from `skill_url`.
  - Exit code is unchanged: always 0. The summary workflow's step does the failing (Task 4).

- [ ] **Step 1: Implement** the items listed under Interfaces.
- [ ] **Step 2: Render four fixtures.** Use `python3 -I coverage/ci-report.py --run "$(jq -nc
  ...)"` with `versioning_mode:"app"` and an `app` object for each case: bumped with an entry;
  not bumped and owed; no CHANGELOG block; plist mismatch. Then render once with
  `versioning_mode:"crates"` and #474's real outputs.
  - Expected: the four app lines exactly as in Interfaces.
  - Expected: the crates-mode render is identical to the summary posted on #474.
- [ ] **Step 3: Commit** `ci summary shows one app version line in app mode`.

### Task 4: Reusable `changes`, `tests`, `audit` and `summary`; vantage calls them

**Files:**
- Create: `.github/workflows/changes.yaml`, `.github/workflows/audit.yaml`,
  `.github/workflows/summary.yaml` (all `on: workflow_call`)
- Modify: `.github/workflows/tests.yaml`
- Modify: `.github/workflows/ci.yaml`, `.github/workflows/post-merge.yaml`
- Modify: `.github/actions/ci-results/action.yml` (paths under `.ci/`)
- Modify: `.github/ci-crates.yaml` `ci_self`: add the three new workflow files

**Interfaces:**
- Consumes: Tasks 1–3.
- **Every shared job** begins with the same steps:
  1. checkout the caller (`persist-credentials: false`);
  2. checkout `repository: ${{ job.workflow_repository }}`, `ref: ${{ job.workflow_sha }}`,
     `path: .ci`;
  3. for cargo jobs only:
     - `env: HAS_SSH_KEYS: ${{ secrets.CI_SSH_KEYS != '' }}`;
     - `webfactory/ssh-agent@dc588b651fe13675774614f8e6a936a468676387` with
       `if: env.HAS_SSH_KEYS == 'true'`;
     - `uses: ./.github/ci-setup` with `if: fromJSON(inputs.setup)` (in tests.yaml, where it
       has the changes outputs) or a `hashFiles('.github/ci-setup/action.yml') != ''` guard;
     - `uses: ./.ci/.github/actions/setup-rust`.
- **`changes.yaml`:**
  - Inputs: `base` (string), `runner` (string, default `ubuntu-latest`), `mode` (`pr` or
    `post-merge`).
  - Secrets: `COVERAGE_EXCEPTIONS_KEY`.
  - Outputs: every `ci-changes` key.
  - Steps:
    - `.ci/script/ci-changes` with labels read live;
    - `.ci/coverage/exceptions.py check` (with the key and `EXCEPTIONS_BASE`);
    - `.ci/coverage/new_tests.py internal` (moved out of vantage's lint).
  - In `post-merge` mode with `warm == full-on-miss`, it checks main's cache key with
    `actions/cache/restore` and `lookup-only: true`, using the same key setup-rust builds. On a
    miss it sets `all=true`.
- **`tests.yaml`:**
  - Inputs: the existing ones, plus `runner`, `build_runner`, `env` and `archive_exclude`.
  - Secrets: `CI_SSH_KEYS`, `COVERAGE_EXCEPTIONS_KEY`.
  - `runs-on: ${{ fromJSON(inputs.runner) }}`. `build` uses `build_runner`.
  - `env` is applied with a first step that writes `GITHUB_ENV` from `fromJSON(inputs.env)`.
  - Archives are uploaded through the fixed slots `ci-tests-1`..`ci-tests-6` (guarded by
    `hashFiles`). Each suite downloads `ci-tests-${{ matrix.group.slot }}`.
  - `archive_exclude` is appended to each group's `-E` filter as `and not (<filter>)`.
  - When `env` sets `CI_TIME: "1"`, the archive loop runs under `/usr/bin/time -v`, so the build
    log shows peak memory (`Maximum resident set size`).
  - nextest config: the caller's tracked `.config/nextest.toml` if `git ls-files --error-unmatch`
    finds it, else `.ci/.config/nextest.toml.example`.
- **`audit.yaml`:**
  - Inputs: `base`, `runner`. Secrets: `CI_SSH_KEYS`.
  - Steps: ssh-agent and setup (needed for `cargo tree`), then `.ci/script/ci-audit`, then
    upload `audit`.
- **`summary.yaml`:**
  - Inputs: `run` (the changes outputs as JSON), `jobs` (`toJSON(needs)` from the caller),
    `runner`.
  - Permissions: contents read, actions read, pull-requests write.
  - Steps: ci-results, then `ci-report` with `--labels` read live, then the comment.
  - Gates:
    - crates mode: today's "Versions bumped" step;
    - app mode: fail when `app.owed && !skip` and the app is not bumped, has no changelog, or
      `!plist_ok`;
    - "every job passed or was skipped".
- **vantage's `ci.yaml`** calls `changes.yaml`, `tests.yaml`, `audit.yaml` and `summary.yaml`
  with `./.github/workflows/...`. Its own jobs (lint, public-api, ci-self, doctests, excluded,
  gated) stay. Lint loses the two coverage steps that moved into changes.
- **`post-merge.yaml`** calls `changes.yaml` with mode `post-merge`, then `tests.yaml` with mode
  baseline.

- [ ] **Step 1: Implement** the items listed under Interfaces.
- [ ] **Step 2: Lint.** `SHELLCHECK_OPTS=--severity=warning actionlint`. Expected: no output.
- [ ] **Step 3: Commit and push** with message `changes, tests, audit and summary are reusable
  workflows that run the shared scripts from .ci`.
- [ ] **Step 4: Verify on GitHub, three runs on the vantage draft PR:**
  1. As pushed (only CI files changed). Expected: `ci-self` runs and the summary is green. The
     ssh-agent and ci-setup steps show as skipped, not failed, in every cargo job.
  2. Push a signed commit that adds a blank line to the top of `vantage-types/src/lib.rs`'s doc
     comment, plus a patch bump and CHANGELOG entry. Expected:
     - only `plain` runs, and the summary has `vantage-types`;
     - the job list matches #474's run;
     - wall time is within ±1 min of #474's comparable run.
  3. Add `ALL_TESTS`. Expected: 4 groups in slots 1–4; every suite downloads only its own slot
     (check each suite's "Download" step size); the summary is green apart from known flakes.
  Then revert the vantage-types commit (signed) and remove the label.
- [ ] **Step 5: Ledger.** Note the wall times of runs 2 and 3 in the ledger.

**Phase A ends.** Report runs 1–3 to the user and wait for "continue".

---

## Phase B: vantage-ui on the shared pipeline (atk4/vantage-ui, branch `shared-ci`)

### Task 5: vantage-ui's thin CI and configuration

**Files (all in /Users/rw/Work/vantage-ui):**
- Create: `.github/ci-crates.yaml` (from `/tmp/ui-ci-crates.yaml` in Task 2; add a `perf:` list
  of crate names whose change runs perf: framework/perf, framework/core, framework/gpui,
  framework/layout, and every crate with a `*_workbench` bin, found with `cargo metadata`)
- Create: `.github/ci-setup/action.yml` (composite; the apt step from today's ci.yaml lines 80–98,
  including the timer stop and `DPKG::Lock::Timeout=180`)
- Create: `.github/workflows/post-merge.yaml`
- Create: `.github/workflows/ci-label.yaml` (a copy of vantage's, plus the `skip-changelog` →
  rerun `summary` and `skip-abi-check` → rerun `plugin-abi-check` rules)
- Create: `coverage/exceptions/.gitkeep`
- Rewrite: `.github/workflows/ci.yaml`
- Delete: `pr-version-check.yaml`, `headless-crates.yaml`, `client-names.yaml`,
  `client-skill.yaml`, `plugin-abi-check.yaml`. Their jobs move into ci.yaml unchanged, apart
  from their triggers.
- Modify: `.github/workflows/port.yaml` (`on: workflow_dispatch` only)

**Interfaces:**
- Consumes: Phase A's workflows at `romaninsh/vantage/.github/workflows/<file>@shared-ci`.
- **`ci.yaml`:**
  - Triggers: `pull_request` to `main`, with no paths filter. `changes` decides what runs.
  - Concurrency: cancel-in-progress.
  - Jobs:
    - `changes` (shared, `runner: ubuntu-latest`);
    - `lint` (local, on `fromJSON(needs.changes.outputs.runner)`; ssh-agent from
      `secrets.CI_SSH_KEYS`, ci-setup, then `cargo fmt --check` and `cargo clippy --workspace
      --all-features --all-targets -- -D warnings`);
    - `tests` (shared);
    - `doctests` (local: `cargo test --doc --workspace --all-features`, then `cargo nextest run
      --workspace --all-features -E 'binary(compile_fail)'` without instrumentation);
    - `audit` (shared);
    - `headless-crates`, `client-names`, `client-skill`, `plugin-abi-check` (moved);
    - `perf` (the existing macOS job, `if:` any of `needs.changes.outputs.crates` is in the
      `perf` list or `all`);
    - `summary` (shared, `needs` every job above).
  - Secrets are passed by name: `CI_SSH_KEYS: ${{ secrets.CI_SSH_KEYS }}` and
    `COVERAGE_EXCEPTIONS_KEY: ${{ secrets.COVERAGE_EXCEPTIONS_KEY }}`.
- **`post-merge.yaml`:** on push to `main`. It calls `changes` (mode `post-merge`), then `tests`
  (mode baseline), plus a local `lint` that warms clippy. It does not touch nightly.

- [ ] **Step 1: Create the branch and draft PR.**
  - Branch `shared-ci` in vantage-ui from `gh/main` (the remote is named `gh` there).
  - Open a draft PR `move pr ci onto the shared vantage pipeline`. Its body links the vantage
    draft PR and says the references point at `@shared-ci` until that PR merges.
- [ ] **Step 2: Implement** the files listed under Files and Interfaces.
- [ ] **Step 3: Lint.** `SHELLCHECK_OPTS=--severity=warning actionlint` in vantage-ui. Expected:
  no output.
- [ ] **Step 4: Commit and push** with message `pr ci runs on the shared vantage pipeline` (signed).

### Task 6: Trial: memory, time and cache on WarpBuild

**Files:** none planned. Fallbacks edit `.github/ci-crates.yaml` only.

- [ ] **Step 1: First run** (the PR as pushed: CI files only). Expected:
  - `changes` reports `crates=[]`. vantage-ui's config has `ci_self: [".github/**"]`, so CI
    files aren't global.
  - The summary posts. The moved checks that run on every PR (headless-crates, client-names,
    client-skill) run.
- [ ] **Step 2: Full trial.** Set `env.CI_TIME: "1"` in the config, push, and add `ALL_TESTS`.
  - Expected: `build` builds every crate instrumented on 8x without OOM.
  - Record: peak memory (`Maximum resident set size` in the build log), build minutes, suite
    minutes, and the summary's covered/dupes/overlaps for all crates.
- [ ] **Step 3: If OOM:**
  1. set `env.CARGO_BUILD_JOBS: "4"` and rerun;
  2. if it still fails, set `build_runner: warp-ubuntu-latest-arm64-16x` and rerun.

  Record each attempt in the ledger.
- [ ] **Step 4: If the `plain` suite runs over 10 minutes,** stop and report to the user with the
  numbers. Splitting the group needs a new `split:` key in `ci-changes`, a vantage-branch change
  the spec left open until measured. Don't build it unasked.
- [ ] **Step 5: Leaf PR check.** Push a signed commit with a one-line doc-comment change in one
  leaf widget crate, plus the app version patch bump, CHANGELOG and plist. Expected:
  - only that crate is tested;
  - wall time is under 10 minutes;
  - the app version line shows `(+patch)`.

  Then revert the commit.
- [ ] **Step 6: Ledger.** Note all timings in the ledger.

**Phase B ends.** Report the trial numbers against the spec's Success section and wait for
"continue".

---

## Phase C: switch to the merged pipeline

### Task 7: Merge vantage, pin vantage-ui, measure

- [ ] **Step 1: Hand over the vantage PR.** Mark it ready for review. The user merges it.
- [ ] **Step 2: Pin vantage-ui.** In vantage-ui, replace every `@shared-ci` with
  `@<vantage merge sha>`, add a `# shared-ci <date>` comment, then commit and push (signed).
  - Expected: `grep -rn "@shared-ci" .github/` is empty.
  - Expected: the run is green.
- [ ] **Step 3: First post-merge run.** After the user merges vantage-ui, watch the first
  post-merge run on main. Expected: `changes` reports a cache miss, sets `all=true`, and the
  `coverage-baseline` artifact holds every measured crate.
- [ ] **Step 4: Next PR.** On the next vantage-ui PR, expect the build log to show a cache hit
  (not "Cache not found") and the summary to compare against main's baseline. Write the Success
  table (leaf time, core time, cache hit, release flows untouched, vantage time unchanged) into
  the ledger and report it.
