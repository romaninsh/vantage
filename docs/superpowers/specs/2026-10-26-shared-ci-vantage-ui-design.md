# Shared CI pipeline, first consumer: vantage-ui

## Why

vantage-ui's PR CI is one job on a WarpBuild arm64 8x runner: `cargo fmt`, then `cargo test` over
all 72 crates. It takes about 20 minutes on every PR, whatever changed:

- about 13–14 minutes of build and test, cold every time. ci.yaml never runs on main, so a PR
  never finds a cache to restore;
- about 5 minutes saving an 8 GB cache that the next PR's first push can't use either.

It failed 6 of its last 10 runs, all in the build-and-test step. Three of those were the same
flaky test (`vantage-acp` `exit_during_turn_ends_turn_and_reports_stderr`). Nothing reports
coverage, duplicate tests, advisories or what a PR changed.

romaninsh/vantage now has a pipeline that does all of that (PRs #473, #474):

- it tests only the crates a PR changed, from one build passed to the test jobs as artifacts;
- it measures coverage and duplicate tests against main's baseline;
- it checks versions and RustSec advisories;
- it posts one summary comment that it edits on every run.

The user wants vantage-ui on the same pipeline for four reasons, in their words:

- it **costs too much**;
- it has **no visibility**;
- it's **too slow**;
- the two pipelines **drift apart**.

The same pipeline will later serve atk4/vantage-lib and atk4/vantage-server. They are not
migrated here, but the shared design must not rule them out (see "Later consumers").

## Goals

1. **One pipeline, maintained in romaninsh/vantage.** vantage-ui calls the shared workflows and
   actions. It keeps only a thin `ci.yaml` and its own configuration. vantage calls the same
   shared pieces the same way, so a fix lands in both.
2. **Test what a PR can break.** Test the changed crates plus every workspace crate that depends
   on them, directly or indirectly (user's choice: "all dependents").
3. **Full coverage in vantage-ui** (user's choice): the `covered`, `dupes` and `overlaps` columns
   and the blocking duplicate-test check, as in vantage.
4. **The summary comment in vantage-ui:** headline, a per-crate table, duplicate findings,
   advisories for the tested crates, and the app version check.
5. **A warm cache for PRs:** a post-merge run on main saves the cache and the coverage baseline
   that PRs restore.
6. **Cheaper and faster than today,** measured on real PRs (see "Success").

## Non-goals

- Migrating vantage-lib or vantage-server (later; see that section).
- Changing vantage-ui's release flow: nightly, draft-release, release, plugin-publish, and the
  macOS signing, notarization and S3 uploads. Those stay repo-local and untouched.
- Moving perf (`script/perf-all`, Metal) onto the shared pipeline. It stays a macOS job in
  vantage-ui.
- Backports. `port.yaml` and the `release/*.*` triggers stay in the repo but are switched off
  (user: keep, don't migrate, disable).
- Fixing vantage-ui's flaky tests. The acp test is a prerequisite tracked on its own (see
  "Prerequisites").
- Enforcing required checks. The private repo on a free plan can't (rulesets return 403). The
  summary still fails visibly.

## Anti-goals

These would make the work a failure even if it ran (user's words, with what each means here):

- **Regressions slip through.** Changed-only testing must not miss a crate that depends on the
  changed one. Dependents expansion is mandatory in vantage-ui. A global change (lockfile,
  toolchain, root Cargo.toml) still tests everything.
- **The release flow breaks.** nightly, draft-release, release and plugin-publish are not edited.
  The only release-adjacent edit is port.yaml's trigger.
- **vantage gets slower.** Making the pipeline shareable must not add wall time or jobs to
  vantage's own runs. Dependents expansion is off in vantage's config. Any new setup hook is
  skipped when unused.

## Current state (inventory, 2026-10-26)

**vantage-ui (atk4/vantage-ui, private)**

- **Workspace:** 72 members in app (6), domain (14), framework (18), widgets (24), plugins (3) and
  dev (6). Excluded from it: `gpui-kit` (absent in CI) and `crates/widgets/extras/map`, which is
  a nested workspace with its own lockfile.
- **Tests:** about 2.5k (nextest count on PR #283). Almost every crate has tests.
  - No database or service is needed. Docker-dependent tests are `#[ignore]` and use fakes;
    loopback networking is enough.
  - No display is needed on Linux; tests use GPUI's `TestAppContext` and `VisualTestContext`.
  - Several `[[test]]` targets have `required-features`, so `--all-features` builds more than CI
    does today.
  - There are trybuild compile_fail tests in framework/component.
- **macOS-only:** perf workbenches (Metal), `crates/plugins/host/tests/handshake.rs` and
  `crates/app/vantage-ui/src/admin/tests/switch.rs`. These already compile out on Linux.
- **Window tests:** on an in-flight branch they assert `NEXTEST` is set (one process per test).
  PR #283 moves CI to nextest with a 5 s slow-timeout kill and adds `.config/nextest.toml`.
- **Memory:** CI sets `CARGO_PROFILE_{DEV,TEST}_DEBUG=line-tables-only`, because full debug info
  in wgpu-linking test binaries ran the runner out of memory.
- **Private git dependencies:** `atk4/vantage-lib` (6 crates) and `atk4/vantage-server` over ssh.
  - Two deploy keys go through `webfactory/ssh-agent`: `VANTAGE_LIB_DEPLOY_KEY` and
    `VANTAGE_SERVER_DEPLOY_KEY`.
  - Cargo needs `CARGO_NET_GIT_FETCH_WITH_CLI=true`.
  - `romaninsh/gpui-component` is public.
- **Build:** `crates/app/vantage-ui/build.rs` calls `script/slice-changelog`, so every build
  needs bash.
- **Versioning:** one app version, in `crates/app/vantage-ui/Cargo.toml` (1.9.8), plus the root
  `CHANGELOG.md` (`## Vantage X.Y` blocks) and the Info.plist strings, all kept equal.
  - `pr-version-check.yaml` enforces this; the `skip-changelog` label opts out.
  - No crate goes to crates.io, but 36 members lack `publish = false`.
- **Runners and cache:**
  - WarpBuild `warp-ubuntu-latest-arm64-8x` (Linux) and `warp-macos-15-arm64-6x` (perf,
    nightly, release).
  - `WarpBuilds/cache@v1` with key `rust-<os>-arm64-<ISO week>-<hash Cargo.lock>`.
  - They moved off GitHub's cache because of its 10 GB limit.
- **Repo-specific checks:**
  - `headless-crates.yaml`: 9 crates must not reach gpui, checked with `cargo tree`;
  - `client-names.yaml`: git grep for client names;
  - `client-skill.yaml`: npm tarball diff;
  - `plugin-abi-check.yaml`: an ABI source change needs an `API_VERSION` bump, with a
    `skip-abi-check` label to opt out;
  - `book.yaml`: mdBook.

**romaninsh/vantage (public), as of #474**

- **`ci.yaml`:** `changes`, `lint`, `public-api`, `tests` (calls `tests.yaml`), `ci-self`,
  `doctests`, `excluded`, `gated`, `audit`, `summary`.
- **`tests.yaml`** (reusable):
  - `build` makes the instrumented nextest archives, one per service group;
  - `matrix-rs`;
  - `suite` runs one job per group: tests, lcov, coverage matrix and duplicate check;
  - `baseline` runs in post-merge mode.
- **Actions:** `setup-rust` (Swatinem rust-cache), `start-services` (vantage's databases),
  `ci-results` (collects artifacts and fetches main's baseline).
- **Scripts:** `script/ci-changes` (diff to crates, groups and versions), `script/ci-audit`,
  `coverage/ci-report.py` (the summary), `coverage/matrix.sh`, `coverage/new_tests.py`,
  `coverage/exceptions.py`, `coverage/matrix-rs`.
- **Hardcoded to vantage:**
  - every job is `runs-on: ubuntu-latest`;
  - `./.github/actions/...` and `script/...` paths resolve in the caller's checkout;
  - `tests.yaml` has fixed upload steps for the groups plain, mongo, surreal and
    mysql-postgres-sqlite;
  - the summary's skill link names romaninsh/vantage;
  - versioning is per crate, assuming `publish != []` means crates.io.

## Design

### The shape

```
romaninsh/vantage (public)                 atk4/vantage-ui (private)
  .github/workflows/  (reusable)
    changes.yaml   ◄──────────────────────── ci.yaml (thin): changes
    tests.yaml     ◄────────────────────────   → tests, audit, lint, own checks
    audit.yaml     ◄────────────────────────   → summary
    summary.yaml   ◄────────────────────────
    tests.yaml (mode: baseline) ◄─────────── post-merge.yaml
  .github/actions/  setup-rust, ci-results   (used only from inside the shared workflows)
  script/ci-changes, script/ci-audit, coverage/*
                                            .github/ci-crates.yaml        (this repo's settings)
                                            .github/ci-setup/action.yml   (apt packages)
```

`changes`, `audit` and `summary` move out of vantage's ci.yaml into reusable workflows. vantage's
ci.yaml calls them exactly as vantage-ui's does. `lint` stays per repo: what it checks differs
(vantage has public-api, repo layers and exceptions; vantage-ui has fmt and clippy).

How the pieces work across repos:

- vantage-ui references the shared workflows by full path and a pinned commit:
  `uses: romaninsh/vantage/.github/workflows/tests.yaml@<sha>`. A private repo may call a public
  repo's reusable workflows.
- Pinning to a commit, not a branch, means a vantage change never breaks vantage-ui unannounced.
  Bumping the pin is a normal vantage-ui PR, and that PR runs the new pipeline before it merges.
- vantage calls the same files with `./.github/workflows/...` at its own commit, so it always
  runs what it ships.

**While the pipeline is being built** (user's instruction), the shared work lives on a branch of
romaninsh/vantage with a draft PR. vantage-ui's migration branch references that branch by name:
`uses: romaninsh/vantage/.github/workflows/tests.yaml@<branch>`.

- A push to the vantage branch changes what the vantage-ui PR runs on its next run, with no pin
  bump in between.
- `job.workflow_sha` resolves to the branch head at run time, so the `.ci/` checkout follows.
- When the vantage PR merges, vantage-ui's references switch from the branch to the merge
  commit's sha in the same vantage-ui PR, before it merges.
- No vantage-ui commit on main ever references a branch.

### Shared tooling reaches the caller's runner

Inside a reusable workflow, `./...` and `script/...` resolve in the caller's checkout, so
vantage-ui would not find `script/ci-changes`. Every shared job therefore does two checkouts:

1. the caller's repo at the PR head, at the workspace root, as today;
2. romaninsh/vantage at `${{ job.workflow_sha }}`, the commit of the shared workflow being run,
   into `.ci/`.

All shared scripts and actions run from `.ci/`: `uses: ./.ci/.github/actions/setup-rust`,
`.ci/script/ci-changes`, `.ci/coverage/matrix.sh`. In vantage, `.ci/` is the same commit as the
workspace, so nothing changes in what runs. The second checkout costs about 2 s. romaninsh/vantage
is public, so it needs no token.

The scripts must work from `.ci/` against the caller's workspace:

- they take the workspace root from the current directory, not from their own location;
- they read the caller's `.github/ci-crates.yaml`, `coverage/config.yaml` and
  `coverage/exceptions`.

Every script that locates the workspace from its own location changes: `script/ci-changes`
(`ROOT = dirname(dirname(__file__))`), `coverage/matrix.sh`, `coverage/new_tests.py`,
`coverage/exceptions.py` and `coverage/config.py`. Each takes the root from `git rev-parse
--show-toplevel` of the current directory. Each reads the caller's coverage config at
`<root>/coverage/config.yaml`; when that file is missing, it uses the shared default at
`.ci/coverage/config.yaml`.

`tests.yaml` hardcodes `cp .config/nextest.toml.example .config/nextest.toml`. Instead it uses the
caller's `.config/nextest.toml` when the caller tracks one (vantage-ui does after #283), else the
example. It requires that config to define a `ci` profile with `junit.path = "junit.xml"`; the
summary reads that file.

### Configuration: `.github/ci-crates.yaml` grows

Each consumer describes itself in its own `ci-crates.yaml`. These are the new keys; the existing
ones (`global`, `ci_self`, `services`, `api_inputs`, `coverage_ignore`, `book`, `excluded`) stay:

```yaml
runner: warp-ubuntu-latest-arm64-8x   # build, matrix-rs, suites; default ubuntu-latest
small_runner: ubuntu-latest           # audit, summary (changes takes a `runner` input instead,
                                      # default ubuntu-latest: it reads this file, so can't use it)
warm: full-on-miss                    # post-merge: changed (default) | full-on-miss
archive_exclude: "binary(compile_fail)"  # nextest filter kept out of the archive
cache: warpbuild                      # warpbuild | rust-cache (default rust-cache)
dependents: true                      # test reverse dependents of changed crates (default false)
versioning:
  mode: app                           # crates (vantage, default) | app (vantage-ui) | none
  crate: vantage-ui                   # app mode: the crate whose version is the app's
  changelog: CHANGELOG.md             # app mode: root CHANGELOG, `## Vantage X.Y` blocks
  heading: "## Vantage {major}.{minor}"
  plist: crates/app/vantage-ui/resources/Info.plist   # optional: both version strings must match
  skip_label: skip-changelog
  # app mode: the bump itself doesn't select the app crate for testing
env:                                   # extra env for every cargo job
  CARGO_NET_GIT_FETCH_WITH_CLI: "true"
  CARGO_PROFILE_DEV_DEBUG: line-tables-only
  CARGO_PROFILE_TEST_DEBUG: line-tables-only
skill_url: https://github.com/romaninsh/vantage/blob/main/coverage/matrix-rs/skills/matrix-based-test-cleanup/SKILL.md
```

How the shared workflows get these values:

- **Runner labels and `env`:** a reusable workflow's `runs-on` can't read a file in the caller.
  So `changes` (which already reads the config) emits them as outputs, and the shared jobs use
  `runs-on: ${{ fromJSON(needs.changes.outputs.runner) }}`. The same goes for `env`.
- **The `setup` hook:** a composite action in the caller's repo, at a fixed path,
  `.github/ci-setup/action.yml`, for apt packages and the like. Every shared cargo job runs it
  with `uses: ./.github/ci-setup` (this resolves in the caller's checkout, as intended) when
  `changes` reports that the file exists.
- **Private git dependencies:** composite actions can't read `secrets`, so the hook can't load
  deploy keys. The shared workflows instead declare optional secrets under
  `on.workflow_call.secrets`:
  - `CI_SSH_KEYS`: the private keys, newline-joined;
  - `COVERAGE_EXCEPTIONS_KEY`.

  How the callers pass them:
  - `secrets: inherit` does **not** cross accounts: atk4 (an org) calling romaninsh (a user
    account) doesn't qualify. So vantage-ui passes each secret by name (`secrets: CI_SSH_KEYS:
    ${{ secrets.CI_SSH_KEYS }}`), and vantage does the same for its exceptions key.
  - `secrets` can't appear in an `if:`. So each shared cargo job sets `env: HAS_SSH_KEYS: ${{
    secrets.CI_SSH_KEYS != '' }}` and runs `webfactory/ssh-agent` with
    `if: env.HAS_SSH_KEYS == 'true'`, before the hook.
  - vantage sets neither the keys nor the hook, so both steps skip.

  vantage-ui's Actions policy is `allowed_actions: all` (checked), so it may call romaninsh's
  workflows.

### Selecting crates: dependents

`ci-changes` gains reverse-dependency expansion when `dependents: true`:

- From `cargo metadata --no-deps`, build the map of workspace members to the workspace members
  they depend on, across normal, dev and build kinds.
- Expand the changed set to its reverse transitive closure.
- `--no-deps` lists each member's dependencies by name and path. That is enough to resolve
  workspace path dependencies without fetching the private git dependencies.

The rest is unchanged:

- `crates` (and `tested`, `groups`, `measured`) are the expanded set.
- `touched` (files changed in the crate itself) stays the basis of the version check.
- `all` still means every crate (label `ALL_TESTS`, a global path, or a lockfile change outside
  the workspace's own entries).

**App-version bumps don't select the app crate.** In app mode, a PR whose only change to the app
crate is its version is not a change to that crate for testing purposes. That covers the version
line in its Cargo.toml, its Cargo.lock entry, Info.plist and the root CHANGELOG. The user put it
this way: "vantage-ui is the app + all the libraries. Coverage is mostly for libraries, we can
probably ignore app version for test purposes." Without this rule, every PR would test the app
crate, because every PR bumps it.

`dependents` defaults to false. vantage keeps its current selection, so vantage is not slowed
down. Whether vantage should turn it on is a separate decision for the user.

### Test groups become data

`tests.yaml`'s `build` job currently has one fixed upload step per vantage group name. It becomes
generic:

- one archive per group, uploaded by a single step in a loop;
- a matrix over `groups` in `suite`, which it already has.

Each group still needs an artifact of its own: `download-artifact`'s `pattern` matches artifact
names, not files inside one. A single combined artifact would make every suite download every
group's archive, and vantage's suites would get slower. So:

- `build` has fixed generic upload slots `ci-tests-1` … `ci-tests-6`. Each is guarded by
  `hashFiles`, as today.
- `changes` numbers the groups and puts the slot number in each group's entry.
- `suite` downloads `ci-tests-<slot>`.
- `ci-changes` fails with a clear error when a config would produce more than 6 groups.
- A group with no services needs no `start-services`: the existing `if` on services stays.

**Ruling:** generic slots over a per-group upload job. Another job would mean another runner
start and an artifact hop.

vantage-ui has no services, so it gets one group, `plain`. It is split by size if the suite runs
too long; see "Open choices for the builder".

### Coverage in vantage-ui

Coverage uses the same instrumented build (cargo-llvm-cov nextest archives) on the 8x runner.
The risk is memory: instrumented wgpu-linking test binaries linked in parallel. The mitigations,
in order:

1. **`line-tables-only` debug:** already set through `env`.
2. **Fewer parallel links:** `CARGO_BUILD_JOBS` from config if the build OOMs.
3. **A bigger WarpBuild runner** (16x) for `build` only, through a `build_runner` key.

The first PR on vantage-ui (task "Trial") measures this before anything else depends on it.

`coverage_ignore` in vantage-ui lists the app crates (`crates/app/*`) and dev tools
(`crates/dev/*`). They are still tested; they just get no `covered` figure. The duplicate check
runs on all tested crates.

The coverage-exceptions key is per repo: vantage-ui gets its own `COVERAGE_EXCEPTIONS_KEY` repo
secret, which the user creates, and its own `coverage/exceptions` file.

### Summary

The summary is `coverage/ci-report.py`, run from `.ci/`. Changes:

- **Versions:**
  - **`crates` mode:** unchanged; there is a per-crate version cell.
  - **`app` mode:** no per-crate version column. A line under the headline reads
    `vantage-ui 1.9.9 (+patch)`, with the CHANGELOG block on hover, or `💤 not bumped` / `📝 no
    CHANGELOG entry` / `🍎 Info.plist says 1.9.8`.
  - The summary job fails on those, unless the `skip_label` is set.
  - **When a bump is owed in app mode** (today's rule, kept): the PR changed anything under a
    workspace crate, the root `Cargo.toml`, or `Cargo.lock`. A dependency-only PR, such as a
    vantage-lib pin bump, still owes a bump. The check is the `## Vantage X.Y` block, as
    pr-version-check's code does today; the `### X.Y.Z` heading its comment mentions was never
    enforced and isn't now.
- **Skill link:** from `skill_url`.
- **Advisories:** unchanged. They are listed for the tested crates and for any the lockfile
  brings in. `cargo audit` runs offline on Cargo.lock, but `ci-audit`'s `cargo tree` needs the
  private git dependencies, so `audit` runs the ssh-agent step too.
- **Run link and repository:** from the `github` context, so they are already right in
  vantage-ui.

### Post-merge

vantage-ui gets `post-merge.yaml` calling the shared `tests.yaml` in `baseline` mode.

Caches save only when their key is new, and vantage-ui's key is week + lockfile hash. So the
first thing main builds in a week is what PRs restore all week. The user chose **a full build on
a key miss**:

- **Key miss on main** (a new week, or a lockfile change): `changes` sets `all` for post-merge.
  It then builds the whole workspace instrumented and runs every suite. That refreshes every
  crate's baseline, and the cache saved at the end is complete.
- **Key hit:** only the merged crates' baseline, as in vantage.
- A repo-local post-merge `lint` job does the same for clippy's cache.
- Cost: about one full build of 8x a week, plus one per lockfile change.
- How `changes` knows: it gets the cache key and checks for a hit with `actions/cache/restore`
  with `lookup-only: true`, or WarpBuild's equivalent.
- This is a `warm: full-on-miss` config key. vantage keeps `warm: changed`, today's behaviour.

It runs on push to main in parallel with nightly and does not touch it. WarpBuild's cache scoping
follows GitHub's: main's entries are visible to PRs. The trial confirms that a PR's build
restores main's cache.

### Checks that need the shared scripts but no build

vantage's `lint` runs `coverage/exceptions.py check` and `new_tests.py internal`. Both are
Python, need no cargo, and must use the same shared commit as everything else. They move into the
shared `changes.yaml` as steps after `ci-changes`. A repo-local lint would need a second,
hand-copied pin that can drift from the `uses:` pin. `lint` itself stays per repo.

### Tests that can't run from an archive

- **Doctests:** a repo-local `doctests` job in each consumer, as vantage already has. vantage-ui's
  runs `cargo test --doc --workspace --all-features` with the ssh-agent and the apt hook.
- **trybuild compile_fail tests** (framework/component): they run cargo at test time. Coverage
  RUSTFLAGS can change their stderr snapshots. They are excluded from the archive with a config
  key, `archive_exclude: "binary(compile_fail)"` (a nextest filter), and run in the same
  repo-local `doctests` job, without instrumentation.
- **Suites:** they get the ssh-agent step and the hook like every shared cargo job. Some tests
  shell out to tools the hook installs.

### Labels

- **Reading labels:** the summary reads labels live through `gh api`, as `changes` already does,
  so `skip_label` takes effect without a new run.
- **`ci-label.yaml`:** each consumer keeps a small copy. A workflow triggered by `labeled` must
  live in the repo that gets the label.
  - `ALL_TESTS` re-runs the whole run, as today.
  - A versioning `skip_label` re-runs only the `summary` job (`gh run rerun --job`).
  - vantage-ui's `skip-abi-check` re-runs only `plugin-abi-check`.

### vantage-ui's own jobs

These stay vantage-ui's own, in its thin `ci.yaml`, and are listed in the summary's `needs`:

- `headless-crates`, `client-names`, `client-skill`, `plugin-abi-check`: moved in as jobs;
  their old workflow files are deleted;
- `perf` on macOS, which runs only when crates that affect perf changed. The `changes` output
  `crates` is matched against a `perf` path list in `ci-crates.yaml`;
- `book.yaml` stays a separate workflow (docs only).

The following go away:

- `pr-version-check.yaml`: app-mode versioning in the summary replaces it;
- the old `ci.yaml` test job.

`port.yaml` and the `release/*.*` triggers: port.yaml's trigger becomes `workflow_dispatch`
only, and `release/*.*` is removed from the `pull_request` branch filters of the jobs that move
into ci.yaml.

### Later consumers (design must allow, not build)

- **vantage-lib:**
  - six crates, `publish = false`, per-crate versions with a rule that dependents bump too
    ("Depends on X");
  - git tags on merge;
  - depends on vantage via crates.io;
  - no secrets;
  - WarpBuild.
  - Fits `versioning: crates` if "published" means "versioned" rather than "crates.io". So the
    version check's crate set comes from a `versioned` list or rule in config, not from
    `publish != []` alone. vantage keeps today's meaning by default.
- **vantage-server:**
  - two crates with one app version (fits `app` mode);
  - needs `CI_SSH_KEYS` for vantage-lib;
  - a Node client job of its own;
  - Docker and npm releases stay local.
- Both have `script/check-repo-layers` with different content. Repo-specific checks stay local
  jobs, as in vantage-ui.

## Prerequisites

1. **vantage-ui PR #283 is merged:** nextest, `.config/nextest.toml`, `cargo test --doc`. The
   shared pipeline runs nextest archives, and the window tests require nextest.
2. **The flaky acp test** `exit_during_turn_ends_turn_and_reports_stderr` is fixed (branch
   `acp-drain-on-exit`) or quarantined. Otherwise the trial can't tell pipeline failures from the
   flake.
3. **vantage's `ci-all-tests-fixes`** (version check on touched crates only; apt hang fix) is
   merged.
4. **Secrets in vantage-ui:** `CI_SSH_KEYS` and `COVERAGE_EXCEPTIONS_KEY`. The user creates them.

## Success

Measured on vantage-ui PRs after the switch, against the 20-minute single job today:

- **Leaf change** (one widget crate): wall time under 10 minutes, with only that crate's suite.
- **Framework-core change:** no slower than today (about 20 min). It tests nearly everything,
  but with a warm cache.
- **Cache:** a PR's first push restores main's cache. The build log shows a cache hit, not
  "Cache not found".
- **Summary comment:** posted and edited in place, with a table for the tested crates,
  dupes/overlaps against main's baseline, and the app version line.
- **Release flow:** nightly, draft-release, release and plugin-publish runs after the switch
  behave exactly as before.
- **vantage:** the median wall time of its PR runs is unchanged within noise (±1 min) on
  comparable PRs, and the job count is unchanged.

## Open choices for the builder

- **Splitting vantage-ui's single group:** split it only if one suite job runs over about 10
  minutes. The split is by crate list (for example `framework`, `widgets`, `rest`), and the
  group name comes from a config `split` key. Not needed until measured.
- **The summary's `needs` list:** the caller's thin ci.yaml owns it. The shared summary takes
  `--jobs` from `toJSON(needs)` as today.
- **Pinning:** vantage-ui pins the shared pipeline by commit sha. Bumping the pin is a manual PR
  until a bot is wanted.
