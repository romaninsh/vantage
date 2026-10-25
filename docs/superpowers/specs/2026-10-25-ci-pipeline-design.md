# CI pipeline: compile once, test what changed

## Why

A PR in `romaninsh/vantage` starts 13 workflows and 17 jobs. On PR #472's last green round that
was about 6.5 minutes of wall time and 31 runner-minutes, and most of it was compiling the same
crates over and over:

| job | time | where it goes |
|---|---|---|
| Coverage | 375s | 225s instrumented compile + run of the whole workspace, `--all-features`, with Postgres, MySQL, Surreal and Mongo; 45s containers |
| Main Workspace | 365s | 31s build, 188s tests (102s compiling test targets, ~86s running; `vantage_docs` doctests alone 34s), then clippy 32s, API client 33s, diorama default build 21s, docs 12s, each a different feature set |
| Kubernetes | 169s | 84s starting minikube |
| SurrealDB, vantage-sql ×4, Mongo | 80–120s each | restore a cache, start a database, build one crate again |
| Repo layers | 103s | 88s in one script |
| SonarQube | 89s | scan |
| AWS, Cmd, Spacetime, Public API, version check | 17–45s each | |

- Coverage already runs every test with every database. Main's test step repeats most of it.
- Test binaries run for ~90s in total. Compiling, cache restores and container startup are the rest.
- The Actions cache holds 11.6 GB against a 10 GB limit, in 24 entries, mostly per-PR copies no
  other PR can read. Evictions cause cold runs: Main peaked at 545s, Build at 877s, Book at 814s.
- Every push to main reruns Coverage, Main, Sonar, Public API, Publish and Book: ~15
  runner-minutes per merge.

Separately, the test-hygiene rules in `coverage/INSTRUCTIONS.md` are enforced only when someone
runs `coverage/matrix.sh` by hand. PR #472 added tests into existing overlap clusters and nothing
stopped it.

## Goals

- Compile each profile once per PR and reuse the result across test suites.
- Run tests only for the crates a PR changed.
- Fail a PR whose new tests duplicate existing ones.
- Run much less after a merge.
- Use what GitHub Actions offers (job graph, outputs, conditional jobs, concurrency, reusable
  workflows, composite actions, cache scoping, label triggers) to make the above cheap.
- Decide between two layouts, A and C below, by measuring both.

## Non-goals

- Cleaning up existing overlap clusters. Clusters made only of tests that already exist on the
  base branch are left alone; that is separate work.
- Paid or larger runners. The repo is public; jobs stay on `ubuntu-latest`.
- Changing what any test asserts.

## Anti-goals

- A changed crate whose tests silently don't run. Skipping is only ever for crates the PR didn't
  touch.
- An agent approving its own duplicate test. Exceptions stay human-only (`coverage/vend-exception.sh`).
- Breaking publish-on-merge.

## Delivery

Three pieces, in order:

1. **Duplicate-test check, validated on history.** Build the check (below) and run it against
   the merge commits of the last five merged PRs (#472, #471 and the three before). Report what
   it flags to the user before wiring it into CI. If it is noisy, adjust the rule or the `jaccard`
   threshold with the user first.
2. **PR 1: layout A** plus everything shared, branched from `main`.
3. **PR 2: layout C**, branched from PR 1 and changing only the test jobs.

Then measure (below), keep the faster, and close the other. If C wins, PR 2 merges after PR 1; if
A wins, PR 2 is closed.

## Shared by both layouts (PR 1)

### `changes` job

Runs first and outputs the list of changed workspace crates as JSON, consumed by later jobs
through `needs.changes.outputs` and `fromJSON`.

- A file belongs to the crate whose directory contains it (longest prefix). Crates are the
  workspace members from `cargo metadata --no-deps` **plus the crates in the root `Cargo.toml`
  `exclude` list** (`vantage-aws`, `vantage-kubernetes`, `vantage-cmd`, `vantage-spacetimedb`,
  `vantage-ui-adapters`, `example_*`), which have their own lockfiles and builds. Any file counts,
  including a version bump in the crate's `Cargo.toml` and its `CHANGELOG.md`. *(Builder-check
  ruling: excluded crates are crates too; otherwise their changes would run nothing.)*
- An excluded crate in the list runs its own suite (build, test, clippy, fmt in its directory),
  replacing `aws.yaml`, `kubernetes.yaml`, `cmd.yaml`, `spacetimedb.yaml`, `ui-adapters.yaml` and
  `ui-examples.yaml` with one reusable suite. They are not part of the shared archive or
  instrumented run.
- `Cargo.lock`: if the only lines that changed are versions of workspace members, it adds
  nothing. Any change to an external dependency means "every crate changed".
- Root `Cargo.toml`, `rust-toolchain*`, `.github/**`, `.config/**`, `coverage/**` (except
  `coverage/exceptions/**`, which only affects the duplicate check) mean "every crate changed".
- `docs4/**` turns on the book job.
- The `ALL_TESTS` label means "every crate changed" and turns on the gated suites.

### Lint job

One job, `--all-features` throughout:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`, over the whole
  workspace, not only changed crates. It compiles in check mode and catches a changed API
  breaking a dependent.
- `cargo doc` with `RUSTDOCFLAGS=-D warnings` for changed crates.
- `script/check-repo-layers` (88s today; find out why before moving it).
- The static half of the duplicate check: no `#[cfg(test)]` module or `src/**/tests*` file added
  in a changed crate.

The narrow feature-set builds go away (`cargo check -p vantage-diorama --no-default-features`,
the separate API client and log-writer runs). The repo is stable enough for `--all-features` to
be the only build.

### Tests for changed crates only

Tests, doctests and coverage run only for crates in the `changes` list. Dependents of a changed
crate don't run their tests by default; clippy covers their compilation and `ALL_TESTS` covers
their tests.

- A database starts only when a crate that needs it is in the list: Postgres and MySQL for
  `vantage-sql` (and anything else whose tests use them), Surreal for `vantage-surrealdb` and
  `surreal-client`, Mongo for `vantage-mongodb`. The mapping lives in one place.
- diorama's `bdd` binary (cucumber, `harness = false`; nextest can't list it) runs through
  `cargo test --test bdd` when diorama is in the list.

- Which crates need which database is derived from today's workflows (`coverage.yaml` services and
  env, `surreal.yaml`, `mongodb.yaml`, `vantage-sql.yaml`), including `bakery_model3` and
  `docs4`.
- Parallelism follows `.config/nextest.toml.example`, which Coverage already uses for an
  all-features workspace run: the `surreal` test group (max 1 thread) covers
  `vantage-surrealdb`, `surreal-client` and `bakery_model3`. Kubernetes keeps
  `--test-threads=1`.

### Gated by the `ALL_TESTS` label

These run when the label is set, or when their own crate is in the `changes` list:

- Kubernetes (minikube).
- AWS (uses repository secrets).
- The MariaDB variant of `vantage-sql`.
- The `bakery_model3` CLI smoke runs.
- `ui-examples`.

`pull_request` triggers on `opened`, `synchronize`, `reopened` and `labeled`. A `labeled` event
for any label other than `ALL_TESTS` does nothing.

### Duplicate-test check

Runs `coverage/matrix.sh <crate>` for each changed crate that has tests, then a checker reads
`clusters-*.md`, `internal.md` and `coverage/exceptions/<crate>.yaml`.

- **New test:** its line range (`file:first-last` in the cluster report) overlaps a hunk changed
  in the PR's diff against its base. Edited tests count as new. The base commit is never built.
- **Fails:** a cluster holding a new test and any other test, whether the new test is the anchor
  or a member; two new tests in one cluster; a new internal test; a rejected exception token.
  If the PR deletes the older test, the cluster disappears and the PR passes.
- **Passes:** clusters made only of tests that already existed.
- **Way out:** only an approved exception. The failure output names the test, its cluster, the
  anchor, and the exact one-line `coverage/vend-exception.sh <package> <test id> "<reason>"`
  command, in the form `INSTRUCTIONS.md` requires.
- Lost-coverage checks against a baseline are out of scope here; the coverage diff against main
  plays that role.

The matrix needs an instrumented build. In A it uses the instrumented archive; in C it reuses the
test-cov job's target directory. `coverage/matrix.sh` may need a mode that runs from an existing
build or archive instead of building.

### Coverage

PR coverage is reported for the changed crates only, compared with main's latest numbers for the
same crates. A whole-workspace percentage computed from a partial run would look like a large drop.

### Required check

A final `ci-ok` job with `if: always()` depends on every job, fails if any of them failed or was
cancelled, and passes when jobs were skipped by design. `main`'s ruleset has no required status
checks today, so removing the old workflows blocks nothing. Making `ci-ok` required is a ruleset
change the user makes (or approves) after the winning layout merges; the PRs don't change repo
settings.

### Branches

PRs into `main` and `v2` run the pipeline, as today. Post-merge runs and cache saves happen on
`main` only; PRs into `v2` restore `main`'s caches. `publish.yaml` (tag-triggered, builds the
legacy `vantage/` crate) stays untouched.

### After a merge (push to `main`)

Only:

- publish-on-merge, as today;
- coverage for the crates the merge commit changed, which becomes the baseline PRs compare
  against and saves the instrumented cache;
- `cargo clippy --workspace --all-targets --all-features`, only to save the lint cache;
- book deploy when `docs4/**` changed.

Main no longer runs the full test suite, Sonar, public-api or the backend workflows.

### SonarCloud

Runs on PRs, and on `main` once a week on a schedule so the PR comparison has a recent base. Not
on every push to main.

### Caching

- Rust caches save only on `main` (`save-if: github.ref == 'refs/heads/main'`); PRs restore only.
  PR caches can't be read by other PRs, so saving them only fills the quota.
- One cache key per profile: normal, instrumented, nightly (public-api). Not one per workflow.
- One set of `RUSTFLAGS` and a CI profile (`CARGO_INCREMENTAL=0`, line-tables-only debug info)
  shared by every job of the same profile, so jobs don't invalidate each other's builds.
- Clearing the existing 11.6 GB of caches is a destructive operation: ask the user before doing it.

### Other Actions settings

- `concurrency` per PR with `cancel-in-progress: true`.
- Shared setup (checkout, toolchain, cache restore, tool install) as a composite action; the
  test suites as a reusable workflow (`workflow_call`) taking the crate list.
- `timeout-minutes` on every job.
- Third-party actions pinned by commit SHA, as `coverage.yaml` and `book.yaml` already do.
- `pr-version-check` and `public-api` stay as they are, except that public-api runs only for
  changed crates that have snapshots.

## Layout A (PR 1): build once, pass the binaries

- A `build` job runs `cargo llvm-cov nextest-archive --workspace --all-features` (instrumented,
  so the run gives coverage and feeds the duplicate check) and uploads the archive as an artifact.
  If `cargo-llvm-cov` can't do this, fall back to `cargo nextest archive` with the coverage
  environment from `cargo llvm-cov show-env` and say so.
- Suite jobs (a matrix built from the changed crates, grouped by the database they need) download
  the archive, start their database, and run their crates with a nextest filterset such as
  `package(vantage-sql)`.
- A final job merges the coverage data and reports it.
- Doctests and `bdd` run in their own job; they can't go through an archive.

## Layout C (PR 2): one instrumented run

- `test-cov`: one job runs `cargo llvm-cov nextest --all-features` for the changed crates, with
  the databases those crates need. The same job runs the duplicate check from its target
  directory.
- Doctests and `bdd` in their own job.
- Gated suites build only their own crate.
- Everything else from PR 1 stays.

## Measuring

For each layout, on a test PR touching `vantage-vista` and on the same PR with `ALL_TESTS`:

- at least three runs on a warm cache, plus one cold (a bumped cache-key prefix). The new cache
  keys and profiles don't exist on `main` yet, so during measurement each PR also saves caches
  from its own branch (`save-if` includes the head ref through a temporary condition). The
  first run seeds the cache and doesn't count, and the condition is removed before merge;
- record wall time from first job start to `ci-ok`, runner-minutes summed over jobs, and
  per-job times.

Keep the layout with the lower median warm wall time. Within 10%, keep the one with fewer
runner-minutes. Report the table to the user before closing either PR.

## Left to the builder

- Where the checker script lives and its language (Python, like the rest of `coverage/`, fits).
- How `matrix.sh` runs from an existing build or archive.
- How octocov (or a replacement) stores and compares per-crate coverage.
- Job and file names, splitting into workflow files, artifact retention (a day is enough).
