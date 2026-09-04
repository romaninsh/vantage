# vantage-rhai: one Rhai host for every YAML script slot

## Problem

Rhai reaches YAML through thirteen separate evaluator families across vantage and
vantage-ui, roughly forty `Engine::new()` sites in non-test code. They split into two kinds
of thing:

- **Vocabularies** that legitimately differ: the SQL builder, the Surreal builder, cmd's
  `run()`, faker verbs, servo, wizard steps, auth `env()`/`jwt()`. These register different
  functions and stay separate.
- **Host plumbing** that should be one thing and is currently reinvented per site: how a
  YAML string is recognised as Rhai, which `${…}` scanner runs, how scope is exposed, whether
  the AST is cached, whether resource limits apply.

The plumbing is where it hurts today:

- Three incompatible `${…}` scanners. The ui-scope one does not handle nested braces, the
  legacy view one does, and the scenery / http ones are textual substitution with no Rhai at
  all. The same YAML string means four different things depending on which key it lands in.
- Only the framework button `action:` sets `max_operations`. Twelve other families can hang
  the UI thread on `while true {}`, including row predicates evaluated per grid row per
  render.
- AST caching is arbitrary: six families cache, the rest re-parse every call.
- vantage-ui pins rhai 1.25 without `serde` (hence a hand-rolled `dynamic_to_json`);
  vantage-cmd pins 1.21 with `serde`.
- Read-set discovery (the mechanism that makes framework pages reactive) exists only in
  ui-scope and is bound to the UI scope graph, so no other site can have it.

## Decisions already made

Recorded from the design conversation so the spec does not relitigate them:

- Rhai is marked in YAML by **key name** for whole-string slots (`when:`, `action:`, `rhai:`)
  and by **`${ expr }` templates** for value-bearing fields (`text:`, `value:`, `label:`).
  No `!rhai` tag.
- Pre-1.0: existing YAML may break; examples and skills docs are migrated in the same change
  that breaks them.
- The host crate lives in **vantage** and is published, so the data-layer engines
  (vantage-vista, vantage-sql, vantage-surrealdb, vantage-cmd, vantage-faker) can adopt it in
  follow-up releases.
- Scenery scripts will read scope names directly (`namespace.value`) instead of through a
  textual `${namespace.value}` pre-pass. That migration belongs to the site-migration spec;
  this crate only has to make it possible.
- Limits: UI-thread evaluations get the ui-scope limits; `spawn_blocking` scripts get a
  larger explicit limit. **There is no unlimited engine.**
- Six Rhai slots with zero YAML uses (table `expressions:`, column `expr:`, column `lazy:`,
  `unit.rhai`, `references.<name>.rhai`, page column `render:`/`copy:`) are deleted rather
  than migrated. Also not this spec.

## Scope of this spec

The `vantage-rhai` crate, its tests, and the day-one adoption by `ui-scope` in vantage-ui as
proof that the abstraction holds. Everything else (`!include`, loader unification,
serde_yaml_ng migration, migrating the remaining evaluator sites, deleting dead slots) is
follow-up work with its own spec.

## Approach

Compiled slot objects over a resolver trait. The crate owns the compiled `Expr`, `Template`
and `Block`; variable lookup goes through a small `Resolver` trait so read-set discovery
lives in the host and works for any site. Vocabularies plug in through a `Vocab` trait.
Every evaluator site becomes: host + vocab + env.

Rejected alternatives:

- **Thin toolkit** (builder + cache + scanner only, evaluation stays at each site). Quick,
  but ui-scope keeps its own `Expr`/`Template` and the divergence shrinks instead of closing.
- **Lift ui-scope wholesale into vantage.** No abstraction needed, but it drags `Source`,
  `Wiring` and reactivity classes into the data layer where nothing needs them.

## 1. Crate and slot types

New workspace member `vantage-rhai`, versioned on the 0.6 line with its siblings.

```toml
[dependencies]
rhai = { version = "1.25", default-features = false, features = ["std", "sync", "serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = { version = "0.8", optional = true }
thiserror = "1"

[features]
default = []
schema = ["dep:schemars"]
```

The crate re-exports `rhai` (`pub use rhai;`). Consumers use the re-export and do not pin
rhai themselves, which is how the 1.21/1.25 and serde-feature drift closes.

Three source-carrying newtypes replace `Option<String>` in YAML structs:

```rust
/// Whole string is one Rhai expression yielding one value.
/// Exactly one `${ … }` wrapper around the whole string is tolerated and stripped.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Expr(String);

/// Literal text with `${ expr }` holes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Template(String);

/// Statements; the result is ignored.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Block(String);
```

They deserialize from a plain string, so a future `!include worker.rhai` that resolves to a
string lands in them unchanged. Each exposes `src(&self) -> &str` and `From<String>` /
`From<&str>` for construction in tests and Rust callers.

Under the `schema` feature each implements `JsonSchema` by hand: `type: string`, a
`description` naming the slot kind ("Rhai expression", "Text with `${…}` Rhai holes",
"Rhai statements"), and an extension key `x-language: rhai`. The marker is what the
scaffolder can read to emit `# language=rhai` above block scalars, and what docs cite.

Compilation is a separate step because it needs a host:

```rust
let expr: Compiled<Expr> = host.compile(&slot)?;   // parse only
let value: Dynamic      = expr.eval(&env)?;         // run
let flag: bool          = expr.eval_bool(&env)?;    // gate form
```

`Compiled<T>` is a single generic struct with a private `kind` marker, so the three slot kinds
share one implementation. Public surface:

| Method | `Expr` | `Template` | `Block` |
|---|---|---|---|
| `eval(&env) -> Result<Dynamic>` | expression value | see §4 | `()` |
| `eval_bool(&env) -> Result<bool>` | yes | no | no |
| `eval_as::<T>(&env)` | yes | yes | no |
| `run(&env) -> Result<()>` | no | no | yes |
| `discover(&env) -> Result<Self>` | yes | yes | yes |
| `read_set() -> &BTreeSet<String>` | yes | yes | yes |
| `is_literal() -> bool` | no | yes | no |
| `src() -> &str` | yes | yes | yes |

Calling a method the kind does not support is a type error, not a runtime one: `eval_bool`
and `eval_as` are implemented on `Compiled<Expr>` and `Compiled<Template>`, `run` on
`Compiled<Block>`, `is_literal` on `Compiled<Template>`.

## 2. Host, engine, limits

```rust
pub struct Host { engine: Engine, cache: AstCache }

let host = Host::builder(Limits::Ui)
    .vocab(ConventionalVocab::new(resolver))
    .vocab(FetchVerbs::new(limit))
    .vocab_fn(|engine| engine.register_fn("money", money))
    .build();
```

`Vocab` is one method:

```rust
pub trait Vocab: Send + Sync {
    fn register(&self, engine: &mut Engine);
}
```

Existing `register_*_onto(engine, …)` functions become `Vocab` impls (a struct holding what
they closed over) with no change to what they register. `vocab_fn` accepts a closure for
one-off registrations so sites are not forced to name a type for two functions.

A host is built once per owner (a component, a table, a wizard run) and is `Send + Sync`
(rhai `sync` feature), so it can be shared across `spawn_blocking` threads as vantage-cmd's
`CompiledScript` does today.

Limits are a closed set. There is no unlimited engine:

| Profile | `max_operations` | string | array | map | call levels |
|---|---|---|---|---|---|
| `Limits::Ui` | 500 000 | 8 MiB | 1 000 000 | 100 000 | 64 |
| `Limits::Background { max_operations }` | caller-set, default 50 000 000 | 8 MiB | 1 000 000 | 100 000 | 64 |

`Limits::Ui` is for anything the UI thread waits on. `Limits::Background` is for anything
under `spawn_blocking`: workers, cmd scripts, imports, faker ticks, MCP data scripts. The
existing `ui_scope::limit_engine` numbers are the source of the `Ui` row; `UI_MAX_OPERATIONS`
moves here.

Both profiles also set `max_expr_depths(256, 256)`, which the SQL and cmd engines set today
and nothing else does.

## 3. Environment, resolver, discovery

Per-evaluation inputs are an `Env`:

```rust
#[derive(Default, Clone)]
pub struct Env {
    vars: Vec<(String, Dynamic)>,
    resolver: Option<Arc<dyn Resolver>>,
}

impl Env {
    pub fn new() -> Self;
    pub fn var(self, name: impl Into<String>, value: impl Into<Dynamic>) -> Self;
    pub fn resolver(self, r: Arc<dyn Resolver>) -> Self;
}
```

Row-scoped sites push `record` / `row` as variables and need no resolver. Sites with a lazy
namespace tree (framework pages, wizard `state`, form fields) provide one:

```rust
pub enum Lookup { Leaf(Dynamic), Namespace, Unknown }

pub trait Resolver: Send + Sync {
    /// Resolve a dotted path. `Namespace` means "keep descending".
    fn resolve(&self, path: &str) -> Lookup;
}
```

Recording is not part of the trait. Discovery (below) wraps whatever resolver the env holds
in a recording adapter, so implementors only answer lookups.

The host installs one `on_var` hook and one `NamespaceProxy` indexer type when it is built.
The hook reads the current `Env`'s resolver out of a hidden scope constant
(`__vantage_env`), so a shared engine serves a different resolver on every call. Pushed
variables shadow the resolver: `on_var` is only consulted for names rhai did not find in
scope. An `Env` with neither a matching variable nor a resolver leaves unknown names to
rhai's own error.

`NamespaceProxy` mirrors ui-scope's `ScopeProxy`: indexing (and `.prop`, which rhai routes to
the indexer when no getter exists) descends namespaces and calls `record` when a leaf is
reached.

Discovery becomes a host feature:

```rust
let expr = host.compile(&slot)?.discover(&env)?;   // one instrumented eval
expr.read_set()                                     // BTreeSet<String>, frozen
```

`discover` wraps the env's resolver in a recording adapter, evaluates once, and stores the
recorded set on the `Compiled<T>`. An unknown name is a `RhaiError::UnknownName` at discovery,
before anything renders, matching current ui-scope behaviour. `Template` discovers each hole
and unions the sets. `Compiled<T>` without a discovery pass reports an empty read-set;
`is_discovered()` tells the two apart.

`discover` on a `Block` runs it once. That is what the form `options:` path does today
(`discover_eval`), and it is documented as such: only use it on blocks whose single execution
is harmless, or run it against a dry-run resolver.

## 4. Template scanner

One scanner, used by `Template` and nowhere else.

- `${` opens a hole. It closes at the matching `}`, counting nested braces, so an inline
  `${ if x { "a" } else { "b" } }` survives.
- Braces inside Rhai string literals within a hole are ignored. Both `"…"` and `'…'` are
  honoured, with backslash escapes.
- An unterminated hole is `RhaiError::Syntax` naming the source and byte offset.
- Text outside holes is literal and passes through verbatim.
- A template with no holes is a literal (`is_literal()`), which is the static-source rule
  framework pages rely on.
- One hole and nothing else keeps the expression's `Dynamic` type.
- Anything else renders text: each hole's value is stringified with rhai's `to_string`
  semantics, unit renders as the empty string.

No escape for a literal `${` in this spec. It has never been possible and nothing needs it.

The ui-scope scanner (`Template::compile`), the legacy view scanner
(`Evaluator::interpolate` + `close_brace`) and the scenery pre-pass (`substitute_scope_reads`)
are all superseded by this one. Env-var substitution in datasource `url:` is not Rhai and
stays where it is.

## 5. Caching

Two layers.

- `Compiled<T>` owns its `Arc<AST>` (one per hole for templates), so hot paths such as
  per-frame template rendering never touch a map.
- `Host` keeps a bounded, source-keyed `AstCache` (`Mutex<HashMap<String, Arc<AST>>>`,
  1024 entries) for sites that receive strings at runtime, such as row predicates evaluated
  per row. On overflow the map is cleared rather than evicted. That is enough for
  config-sized script sets and keeps the code small.

`host.compile(&slot)` consults the cache; `host.compile_uncached` bypasses it for one-off
scripts (MCP data scripts) so they do not churn it. Cache hits are safe across vocabularies
because rhai binds functions at evaluation, not at parse.

## 6. Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum RhaiError {
    #[error("{0}")] Syntax(Located),
    #[error("unknown name `{path}` in scope")] UnknownName { path: String, src: String },
    #[error("{0}")] Runtime(Located),
    #[error("expected {expected}, got {actual}")] WrongType { expected: &'static str, actual: String, src: String },
    #[error("script exceeded its {limit} limit")] LimitExceeded { limit: &'static str, src: String },
}

pub struct Located { src: String, line: usize, column: usize, message: String }
```

`Located`'s `Display` prints the message, then the offending source line with a caret under
the reported column. Every variant carries the slot source so the catalog validator can point
at the YAML file and key. `LimitExceeded` is split out from `Runtime` by matching rhai's
`ErrorTooManyOperations`, `ErrorDataTooLarge` and `ErrorStackOverflow`, so the UI can say
"script exceeded its operation budget" instead of a generic failure.

## 7. Value conversion

With rhai's `serde` feature on, the crate exposes:

```rust
pub fn to_json(value: &Dynamic) -> serde_json::Value;
pub fn from_json(value: &serde_json::Value) -> Dynamic;
```

built on `rhai::serde::{to_dynamic, from_dynamic}`. These replace the hand-rolled converters in
vantage-ui's actions http path and in vantage-vista's `convert.rs` when those sites migrate.
ui-scope keeps its own `Value` conversion because that type carries reactivity metadata.

## 8. Day one

### In vantage

- Add the crate to the workspace, with the tests in §9.
- `CHANGELOG.md` entry.
- Publish `vantage-rhai 0.6.0`. No other vantage crate changes in this step.

### In vantage-ui (proof of the abstraction)

`ui-scope` adopts the crate:

- `Cargo.toml`: depend on `vantage-rhai`, drop the direct rhai dependency, re-export
  `vantage_rhai::rhai` in place of the current `pub use rhai`.
- Implement `Resolver` for `Arc<Scope>` (`resolve` = `lookup` / `is_namespace`).
- Delete `expr.rs`'s `build_engine`, `ScopeProxy`, `proxy_index`, the `Template::compile`
  scanner, and `script.rs`'s `limit_engine` / `UI_MAX_OPERATIONS`.
- `ui_scope::Expr` and `ui_scope::Template` become thin wrappers over `Compiled<Expr>` /
  `Compiled<Template>` plus the frame, keeping their current signatures
  (`compile(src, &frame)`, `eval() -> Value`, `read_set()`, `is_literal()`,
  `Template::expression`) so no consumer in vantage-ui changes.
- `discover_eval` becomes `host.compile(Block).discover(env)` with the caller's vocab.
- `ScriptEnv` becomes a `Compiled<Block>` over a `Limits::Ui` host, with the write proxy
  registered as a `Vocab`.

Existing ui-scope tests must pass unchanged. That is the acceptance test for this spec.

Migrating the other eleven vantage-ui sites and the five data-layer engines is the next spec.

## 9. Tests (in the crate)

Scanner:
- nested braces inside a hole
- braces inside `"…"` and `'…'` literals inside a hole, including escaped quotes
- unterminated hole reports source and offset
- no holes → `is_literal()`; one hole → typed result; mixed → string, unit renders empty

Slot kinds:
- `Expr` strips exactly one `${…}` wrapper; two adjacent holes are an error, not a wrapper
- `eval_bool` on a non-bool is `WrongType`
- `Block` result is ignored; `run` succeeds on a block ending in an expression

Limits:
- `while true {}` under `Limits::Ui` returns `LimitExceeded { limit: "operations" }`
- string doubling loop hits `LimitExceeded { limit: "size" }` under both profiles
- `Limits::Background { max_operations }` honours the caller's number

Discovery:
- read-set equals the leaf paths touched; namespace descent records the full dotted path
- unknown name fails at `discover`, not at `eval`
- pushed variables shadow the resolver
- `read_set()` is empty and `is_discovered()` false before discovery

Cache:
- same source compiles once (assert via a counting `Vocab` or `Arc::ptr_eq` on the AST)
- overflow past the bound clears and continues to serve

Sync:
- one `Host` shared by two threads evaluates concurrently with distinct `Env`s and gets the
  right values each

Errors:
- syntax error `Display` shows the offending line and caret
- runtime error carries the slot source

Value conversion:
- round-trip a nested map/array/int/float/bool/unit through `to_json` / `from_json`

## Non-goals

- `!include` resolution and the loader pass (separate spec).
- serde_yaml → serde_yaml_ng migration (separate spec).
- Migrating any evaluator site other than ui-scope.
- Deleting the six dead slots.
- Escaping a literal `${` in templates.
- Editor highlighting comments in scaffold templates (needs the `x-language` marker from
  this spec; the emission belongs to the loader/scaffold spec).
