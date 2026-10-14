# Test hygiene: instructions for agents

These tools find overlapping tests and lost coverage in one crate. This file says how to use them
and the rules for removing and writing tests. A task brief tells you which crate, which step, and
where to write your report. Where a brief and this file differ, the brief wins.

## The tools

```
coverage/matrix.sh [--baseline] <package> [jaccard]     # or: mise run test-matrix <package>
```

- **What it does.** It builds `<package>`'s tests instrumented, in their own target dir, so your
  regular build is untouched. It runs **every test alone** with its own coverage profile and records
  the code regions under `<crate>/src` that each test covers.
- **Running it.** Run it as is: it writes its own log, so don't redirect or pipe it. It takes about a
  minute per few hundred tests. Tests that need a database need it running, as for any test run.
- **`--baseline`** saves the run as `coverage/report/<package>.baseline/`. Every later run compares
  against it.
- **`jaccard`** (default 0.9) is how much two tests must share to count as overlapping. You won't
  normally change it.

Never write your own tooling for this. If the reports don't tell you something you need, say so in
your report.

## The reports

Everything goes to `coverage/report/<package>/`, which is git-ignored and regenerated on every run.

| file | what it holds |
|---|---|
| `index.md` | totals, the file list, and blackbox tests with no unique region outside any cluster |
| `internal.md` | every test module and test file under `src/`, then every internal test, with line ranges |
| `clusters-NN.md` | overlapping **blackbox** tests, 8 clusters per file, each test a row with an empty `decision` cell and a `reason:` line per cluster |
| `lost.txt` | regions covered in the baseline but no longer, by file and line, the biggest losses first (only when a baseline exists) |
| `run.log` | the build and test output |

(`coverage/report_html.py` renders the same analysis as HTML for people. You don't need it.)

Words the reports use:

- **Internal test:** its source is under `src/`: an inline `#[cfg(test)]` module, or a
  `src/**/tests*` file. It exercises internals.
- **Blackbox test:** its source is under `tests/`, and it uses only the crate's public API.
  Public test-support API counts as public: `mocks`, and anything behind a test-support feature.
- **Region:** a span of code under `src/`. `regions` is how many a test covers.
- **`unique`:** the regions no other blackbox test covers.
- **Cluster:** blackbox tests whose regions are identical, or one inside another, or shared at least
  `jaccard`. The first row is the cluster's largest test, the **anchor**.
- **`beyond anchor`:** the regions a test covers that the anchor doesn't.

**Every location is a line range, `file:first-last`,** attributes and doc comments included. Read only
those lines. Don't read whole files.

**Decision files.** The cluster files are regenerated on every run. Copy them to the directory your
brief names before filling them in, and record decisions only in the copies.

## Exceptions

`coverage/exceptions/<package>.yaml` lists tests a human approved to keep regardless of overlap: a
regression test, a test the book names, and so on. Each entry has a `test`, a `reason` and a signed
`token`.
- The matrix pre-marks an approved test `EXEMPT` in the cluster files. An EXEMPT test always stays.
  A test whose regions all fall within the cluster's EXEMPT tests (`beyond exempt` 0) is deleted.
- `index.md` lists accepted and rejected exceptions. An entry is rejected when its token is forged,
  reused, or no longer matches the test and reason.

You can't create an exception. Only `coverage/vend-exception.sh` can, and only a human runs it. When
you believe a test needs one, don't delete it. List it in your report with the exact command you
propose, and explain why:

```
coverage/vend-exception.sh <package> <test id as in the cluster file> "<reason>"
```

The command must stand alone: one call, from the repo root, with no pipes, redirects, `&&`/`;`
chaining, subshells or `$(...)`. Anything else is refused, because the user must be able to read
the test and reason they're approving.

Never edit an exceptions file.

## Package settings

`coverage/config.yaml` holds two settings per package. Only the user changes this file.

- **`exclude`:** paths the matrix ignores, such as vista's `src/mocks/**` test fixture. Their
  regions count neither as covered nor as lost, so never write a test to cover them. `index.md`
  shows what is left out.
- **`tolerance`:** how many lost regions count as run-to-run noise. `lost.txt` says whether a run
  is within it.

## Asking the user

When a case is genuinely puzzling, ask the user with the `AskUserQuestion` tool instead of guessing:
the docs and sibling APIs disagree, or don't settle what a test should expect, or you can't tell
whether a region is dead code or an API gap. Ask one question per case: the item, what you found,
the options, and the one you'd pick. If you don't have the tool, stop that case and put the
question at the top of your report. Don't ask about anything these instructions already decide.

## Removing tests

1. **Internal tests are disqualified.** Delete every test listed in `internal.md`, however good it is.
   Coverage comes back through the public API afterwards.
   - Delete each module or file in its table.
   - Delete the `#[cfg(test)] mod tests;` line pointing at a deleted file, and directories left empty.
   - Delete test-only helpers, imports and `#[cfg(test)]` items the deletions leave dead. If non-test code
     uses a `#[cfg(test)]` item, leave it and report it.
2. **Keep exactly one test per cluster.** Read each member's line range, then put `KEEP` on one row and
   `DELETE` on the rest, with a one-line reason. In a cluster with `EXEMPT` rows, first mark
   `DELETE` on every row whose `beyond exempt` is 0. The rows left over get exactly one `KEEP`
   between them, chosen the same way, or none if no rows are left over.
   - Keep the test whose assertions say the most about behaviour.
   - If two are equal, keep the one covering more regions.
3. **Never delete a test the book names.** The book marks the test behind an example with
   `<!-- tested: <test path> -->` in `docs4/src`. Those tests are vended as exceptions. If you find a
   marked test that isn't `EXEMPT`, keep it and propose an exception for it.
4. **Only remove.** Don't merge tests, rewrite assertions or add tests while removing. Tests outside any
   cluster stay.
5. **Never change non-test code** in this step.

## Writing tests: contract first

When you write or restore a test, it checks behaviour the public API **promises**, not behaviour the
code happens to have.

1. **Name the behaviour:** the public item, the input, the promise. The test's name states that
   behaviour, and matches what it asserts: a test of `FilterOp::Gt` isn't named for equality.
2. **Find the contract before opening the implementation:**
   - the item's doc comment;
   - the book (`docs4/src`; `docs4/src/migration-2.0/` for what changed in 2.0);
   - how sibling APIs behave in the same situation. Behaviour must be consistent.
3. **Write the expected outcome from the contract.** If the docs are silent, choose what a user would
   expect, keep it consistent with neighbouring behaviour, and record the choice in your report.
   If more than one outcome is reasonable, ask the user (see "Asking the user").
4. **Run it.** If it fails, the code disagrees with the contract. Don't change the assertion to match
   the code: mark the test `#[ignore = "possible bug: <one line>"]` and report it.
5. **Never read the implementation to decide what an assertion should say.** You may read it to find
   *which* public entry point reaches a lost region, but not to learn the expected value.

## Restoring coverage

Restoring is done when `lost.txt` says the run is within the package's tolerance. Within it, stop:
don't chase the last few regions.

- **Find what was lost.** Run `coverage/matrix.sh <package>` and work through `lost.txt` file by file:
  read the lost line ranges, find the public entry point that reaches them, and write tests contract
  first.
- **Where tests go.** Put a test in the existing `tests/` file for its area. Make a new file only when no
  area fits.
- **Regions the public API can't reach** stay uncovered. Report them by file and lines: they're dead
  code or an API gap, and the user decides which: ask when you can't go on without a decision. Never
  make an item `pub` to reach one.
- **Re-run as you go.** Re-run the matrix every file or two to see what is still lost.

## Always

- **Edits:** Edit/Write only, no sed or scripted rewrites.
- **No commits, no subagents.**
- **cargo:** every run uses `--all-features`. Write cargo output to a file and read the file; don't pipe
  it through grep, head or tail.
- **Before you finish:**
  `cargo clippy -p <package> --all-features --all-targets -- -D warnings`
  passes, and the matrix reports `failed/unmeasured 0`, apart from tests marked as possible bugs.
