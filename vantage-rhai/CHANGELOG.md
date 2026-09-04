# Changelog

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
