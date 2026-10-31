# Changelog

## 1.3.0 — 2026-10-30

- `register_atomic_syntax(engine, eval)`: the grammar of `atomic { BODY }` and
  `atomic { BODY } rollback (NAME) { ARM }`, defined once. vantage-vista's `AtomicVocab` registers
  it with the block's behaviour; a validator registers it with an `eval` that never runs, so a body
  using `atomic {` compiles there too.

## 1.2.0 — 2026-10-28

- `Compiled<Block>::eval_raw(&env)`: `eval` with rhai's own `EvalAltResult`, before it becomes a
  `RhaiError`. A `throw` keeps its thrown `Dynamic` and the position of its `throw` in `ast()`, so
  a host can tell a script's `throw` from a library's error, which is an `ErrorRuntime` too but
  sits at the call that raised it.

## 1.1.3 — 2026-10-26

- Tests of one behaviour are folded into one test each (template rendering, operation ceilings,
  the compile cache, namespaces, the row pipe, the template scanner) and tests another test already
  covers are dropped: 204 tests become 160 with the same coverage. No API or behaviour change.

## 1.1.2 — 2026-10-25

- Tests whose assertions other tests already make are dropped. No API or behaviour change.

## 1.1.1 — 2026-10-14

- The in-crate unit tests move to a blackbox suite under `tests/`, written from the README and the
  book. No API or behaviour change.
- The README now says whitespace around a lone `${ … }` hole keeps the hole's value; only visible
  text turns the template into a `String`.

## 1.1.0 — 2026-10-14

- `Namespace`, a builder for registering a named module of functions and variables on an engine.
  Its functions answer only qualified calls (`ns::name(..)`), and its variables read as
  `ns::name`.
- Requires rhai `>=1.25, <1.27` with the `internals` feature: vantage-vista walks rhai's AST to
  refuse dropped table handles, and that AST may change in a rhai minor release.

## 1.0.1 — 2026-10-05

- The `invalidation` example uses `AtomicU64::try_update` (Rust 1.99 deprecates `fetch_update`).

## 1.0.0 — 2026-10-03

- Stable release. The API hasn't changed since 2026-09-27 (1 week); no code changes.

## 0.6.4 — 2026-09-27

- `RowPipe`: a long-lived script that reads rows with `next_row()` and
  answers each with `emit(...)`, under a per-row operation budget. A script
  that throws is restarted.

## 0.6.3 — 2026-09-19

- A template holding one `${ … }` hole surrounded only by whitespace keeps
  the hole's value (a map, a number) instead of rendering it to text — the
  newline a YAML block scalar leaves after the hole no longer turns a
  `#{ text, color }` label into its debug string.

## 0.6.2 — 2026-09-05

Additions made while the data layer and vantage-ui moved onto the host.

- `Compiled<Block>` gains the value methods (`eval`, `eval_as`, `eval_bool`)
  next to `run`: a script whose last statement is its answer no longer needs
  to be re-typed as an expression.
- `Compiled::engine()` and `Compiled::ast()` expose the compiled pair for
  the one caller shape the host cannot express — evaluating to a `FnPtr`
  once and calling it per line (a log filter).
- The slot types (`Expr`, `Template`, `Block`) implement `Deref<Target =
  str>`, `AsRef<str>`, `Display`, `Default`, `PartialEq<str>` and
  `PartialEq<&str>`, so a YAML field can change from `Option<String>` to
  the slot without touching readers that only display or compare it. The
  kind still matters where it counts: `Host::compile` takes the typed slot.
- Parse errors keep rhai's `Syntax error:` prefix in `Display`, matching
  the fixtures downstream crates assert against.

## 0.6.1 — 2026-09-04

Review fixes, all with regression tests.

- The `${…}` scanner now skips everything rhai would not read as code:
  backtick literal strings, char literals, and `//` / `/* */` comments.
  Before, a backtick string containing a brace either failed to compile with
  a misleading "unterminated" message or split the template in the wrong
  place — `${ `{` }` is valid rhai and was rejected.
- A runtime error inside a template hole now reports the hole's position
  **in the template**, not in the hole's own isolated source, and prints that
  position once rather than twice. A fault on line 2 of a block scalar used to
  point at line 1, column 1.
- A failure inside a script `fn` arrives wrapped in `ErrorInFunctionCall`;
  the wrapper is now unwrapped before classification, so an unknown name in a
  helper function is `UnknownName` rather than an opaque `Runtime`.
- `Resolver::resolve` is called once per name per evaluation, not twice. A
  resolver that counts, logs, or caches on read no longer sees doubled reads.
- One `rhai::Scope` per evaluation instead of one per template hole.
- `Host::ast` takes a `Mode` instead of a caller-built cache key, so an
  expression and a script with identical text cannot alias in the cache.
- Template scanner columns are counted in chars, matching rhai's own
  positions; non-ASCII text before a hole no longer shifts the caret.
- `Limits::Background { max_operations: 0 }` no longer means *unlimited*.
  Rhai reads a zero ceiling as "no limit", which quietly undid the one
  guarantee the type exists to make; the floor is now 1.
- A syntax error in a template hole reports the template's line, column and
  source rather than the hole's, so a validator can still name the YAML key.
- `to_json` walks maps and arrays instead of serializing the root in one go.
  A single unsupported value nested in a map used to flatten the entire
  object into one string; now it costs only its own leaf. Non-finite floats,
  which JSON cannot represent, render as text.
- `Compiled` implements `Debug`.

## 0.6.0 — 2026-09-04

Initial release. One Rhai host for every YAML script slot, so the pieces that
should not differ per call site — how a string is recognised as Rhai, which
`${…}` scanner runs, whether an AST is cached, whether a resource limit
applies — live in one place. Domain vocabularies stay separate and plug in
through `Vocab`.

- `Expr` / `Template` / `Block`: source-carrying slot types for YAML struct
  fields, deserialized transparently from a plain string. Under the `schema`
  feature each emits a string schema carrying `x-language: rhai`.
- `Host`: one configured `rhai::Engine` plus a bounded (1024-entry) AST cache,
  built once per owner and `Send + Sync`. `compile` caches, `compile_uncached`
  does not.
- `Limits`: a closed set of profiles. `Ui` bounds operations at 500_000 for
  anything the UI thread waits on; `Background` carries a caller-set budget
  (default 50_000_000) for `spawn_blocking` work. Both bound string, array and
  map sizes, call depth and expression depth. There is no unlimited engine.
- `Resolver` + `Lookup`: lazy resolution of dotted paths against a namespace
  tree, with `Compiled::discover` recording every leaf read and freezing the
  result as the slot's read-set.
- One `${…}` scanner that counts nested braces and skips braces inside string
  literals, so `${ if x { "a" } else { "b" } }` parses.
- `RhaiError`: syntax, unknown-name, runtime, wrong-type and limit-exceeded
  variants, each carrying the slot source; syntax and runtime errors print the
  offending line with a caret.
- `to_json` / `from_json` over rhai's serde support.
