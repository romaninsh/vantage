# Changelog

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
