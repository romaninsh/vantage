# vantage-rhai Host Crate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a published `vantage-rhai` crate that owns Rhai host plumbing (slot types, limits, vocab/resolver traits, discovery, one `${}` scanner, AST cache, located errors) and prove it by making vantage-ui's `ui-scope` run on it with its tests unchanged.

**Architecture:** Three source-carrying slot newtypes (`Expr`, `Template`, `Block`) are compiled by a `Host` (a shared `rhai::Engine` with closed `Limits` plus a bounded AST cache) into `Compiled<S>` objects that evaluate against an `Env` (pushed variables plus an optional lazy `Resolver`). Read-set discovery wraps the env's resolver in a recording adapter. Vocabularies plug in through `Vocab`. ui-scope keeps its public API and becomes a thin wrapper.

**Tech Stack:** Rust edition 2024, rhai 1.25 (`std`, `sync`, `serde`), serde, serde_json, thiserror 2, schemars 0.8 (optional feature `schema`).

**Spec:** `/Users/rw/Work/vantage/plans/2026-09-04-vantage-rhai-host-design.md`

## Global Constraints

- rhai pinned `1.25`, `default-features = false`, features `["std", "sync", "serde"]`. Consumers use `vantage_rhai::rhai`, never their own pin.
- No unlimited engine: every `Host` applies `Limits::Ui` or `Limits::Background { .. }`; both set sizes (8 MiB string, 1 000 000 array, 100 000 map), 64 call levels, `max_expr_depths(256, 256)`. `Ui` ops = 500 000; `Background` default ops = 50 000 000.
- `Expr` strips exactly one `${ … }` wrapper; two adjacent holes are a syntax error.
- Template: brace-nesting aware, string-literal aware (`"…"` and `'…'` with backslash escapes), unterminated hole is a syntax error, no holes → literal, one hole alone → typed, otherwise text with unit rendered as empty.
- AST cache bound: 1024 entries, cleared on overflow.
- Error variants: `Syntax`, `UnknownName`, `Runtime`, `WrongType`, `LimitExceeded`; every variant carries the slot source.
- vantage repo commit hook: **single-line commit messages, no attribution trailers**. Design docs live in `plans/`.
- Existing `ui-scope` tests in vantage-ui must pass unchanged after adoption.
- vantage-ui consumes vantage crates from crates.io. During development ui-scope path-depends on `../vantage/vantage-rhai`; the last task re-pins to the published version.

## File Structure

vantage repo, new crate `vantage-rhai/`:

| File | Responsibility |
|---|---|
| `Cargo.toml` | manifest, features |
| `src/lib.rs` | re-exports only |
| `src/slot.rs` | `Expr`, `Template`, `Block` newtypes; serde; optional `JsonSchema` |
| `src/error.rs` | `RhaiError`, `Located`, conversions from rhai errors |
| `src/limits.rs` | `Limits` enum and `apply` |
| `src/host.rs` | `Host`, `HostBuilder`, `Vocab`, `AstCache` |
| `src/template.rs` | scanner: `Part`, `split` |
| `src/resolver.rs` | `Resolver`, `Lookup`, `EnvHandle`, `NamespaceProxy`, `RecordingResolver`, `install` |
| `src/compiled.rs` | `Env`, `Compiled<S>`, `Slot` sealed trait, eval/discover |
| `src/json.rs` | `to_json`, `from_json` |
| `CHANGELOG.md` | entry (repo root) |

vantage-ui repo, modified:

| File | Change |
|---|---|
| `framework/ui-scope/Cargo.toml` | depend on vantage-rhai, drop rhai |
| `framework/ui-scope/src/lib.rs` | re-export `vantage_rhai` and its `rhai`; drop `limit_engine`/`UI_MAX_OPERATIONS` |
| `framework/ui-scope/src/expr.rs` | `FrameResolver`; `Expr`/`Template`/`discover_eval` as wrappers |
| `framework/ui-scope/src/script.rs` | `ScriptEnv` over `Host` + `Block` |
| `crates/wizard/src/runtime.rs:366` | replace `limit_engine` call |

---

### Task 1: Crate scaffold and slot types

**Files:**
- Create: `vantage-rhai/Cargo.toml`, `vantage-rhai/src/lib.rs`, `vantage-rhai/src/slot.rs`
- Modify: `Cargo.toml` (workspace `members`)

**Interfaces:**
- Produces: `Expr(String)`, `Template(String)`, `Block(String)` with `src(&self) -> &str`, `From<&str>`, `From<String>`, `Deserialize`/`Serialize` transparent; feature `schema` adds `JsonSchema` with `x-language: rhai`.

- [ ] **Step 1: Create the manifest and add the workspace member**

`vantage-rhai/Cargo.toml`:

```toml
[package]
name = "vantage-rhai"
version = "0.6.0"
edition = "2024"
license = "MIT OR Apache-2.0"
description = "One Rhai host for every Vantage YAML script slot: slot types, limits, vocab/resolver traits, discovery, template scanner"
repository = "https://github.com/romaninsh/vantage"

[lib]
doctest = false

[features]
default = []
schema = ["dep:schemars"]

[dependencies]
rhai = { version = "1.25", default-features = false, features = ["std", "sync", "serde"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1"
thiserror = "2.0"
schemars = { version = "0.8", optional = true }
```

In the root `Cargo.toml`, add `"vantage-rhai",` to `members` directly after `"vantage-vista",`.

- [ ] **Step 2: Write the failing slot tests**

`vantage-rhai/src/slot.rs`:

```rust
//! Source-carrying slot types. A YAML struct field holds one of these instead
//! of `Option<String>`; the kind says how the host compiles it.

use serde::{Deserialize, Serialize};

macro_rules! slot {
    ($(#[$doc:meta])* $name:ident, $desc:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn src(&self) -> &str {
                &self.0
            }
            pub const DESCRIPTION: &'static str = $desc;
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        #[cfg(feature = "schema")]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> String {
                stringify!($name).to_string()
            }
            // `gen` is a reserved keyword in edition 2024, hence the raw identifier.
            fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
                use schemars::schema::{InstanceType, Metadata, SchemaObject};
                let mut obj = SchemaObject {
                    instance_type: Some(InstanceType::String.into()),
                    metadata: Some(Box::new(Metadata {
                        description: Some($desc.to_string()),
                        ..Default::default()
                    })),
                    ..Default::default()
                };
                obj.extensions
                    .insert("x-language".to_string(), serde_json::Value::String("rhai".to_string()));
                obj.into()
            }
        }
    };
}

slot!(
    /// Whole string is one Rhai expression yielding one value. Exactly one
    /// `${ … }` wrapper around the whole string is tolerated and stripped.
    Expr,
    "Rhai expression"
);
slot!(
    /// Literal text with `${ expr }` Rhai holes.
    Template,
    "Text with `${…}` Rhai holes"
);
slot!(
    /// Rhai statements; the result is ignored unless read via `eval`.
    Block,
    "Rhai statements"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Doc {
        when: Expr,
        text: Template,
        action: Block,
    }

    #[test]
    fn slots_deserialize_from_plain_strings() {
        let doc: Doc = serde_json::from_str(
            r#"{"when":"row.x > 1","text":"Hi ${row.name}","action":"state.done = true;"}"#,
        )
        .unwrap();
        assert_eq!(doc.when.src(), "row.x > 1");
        assert_eq!(doc.text.src(), "Hi ${row.name}");
        assert_eq!(doc.action.src(), "state.done = true;");
    }

    #[test]
    fn slots_serialize_transparently() {
        let s = serde_json::to_string(&Expr::from("a + b")).unwrap();
        assert_eq!(s, r#""a + b""#);
    }

    #[cfg(feature = "schema")]
    #[test]
    fn schema_carries_language_marker() {
        let schema = schemars::schema_for!(Expr);
        let json = serde_json::to_value(&schema).unwrap();
        assert_eq!(json["type"], "string");
        assert_eq!(json["x-language"], "rhai");
        assert_eq!(json["description"], Expr::DESCRIPTION);
    }
}
```

`vantage-rhai/src/lib.rs`:

```rust
//! One Rhai host for every Vantage YAML script slot.
//!
//! Consumers use the re-exported [`rhai`] so every crate shares one version
//! and feature set.

pub use rhai;

mod slot;

pub use slot::{Block, Expr, Template};
```

- [ ] **Step 3: Run the tests to verify they compile and pass**

Run: `cd /Users/rw/Work/vantage && cargo test -p vantage-rhai && cargo test -p vantage-rhai --features schema`
Expected: 2 passed (default), 3 passed (schema).

- [ ] **Step 4: Commit**

```bash
cd /Users/rw/Work/vantage
git add Cargo.toml Cargo.lock vantage-rhai
git commit -m "vantage-rhai: crate scaffold with Expr/Template/Block slot types"
```

---

### Task 2: Errors

**Files:**
- Create: `vantage-rhai/src/error.rs`
- Modify: `vantage-rhai/src/lib.rs`

**Interfaces:**
- Produces: `RhaiError` enum, `Located { src, line, column, message }`, `RhaiError::from_parse(src, rhai::ParseError)`, `RhaiError::from_eval(src, Box<rhai::EvalAltResult>)`, `type Result<T> = std::result::Result<T, RhaiError>`.

- [ ] **Step 1: Write the failing tests**

`vantage-rhai/src/error.rs`:

```rust
//! One error type for every slot. Every variant carries the slot source so a
//! validator can point at the YAML key that produced it.

use rhai::{EvalAltResult, ParseError, Position};

pub type Result<T> = std::result::Result<T, RhaiError>;

#[derive(Debug, thiserror::Error)]
pub enum RhaiError {
    #[error("{0}")]
    Syntax(Located),
    #[error("unknown name `{path}` in scope")]
    UnknownName { path: String, src: String },
    #[error("{0}")]
    Runtime(Located),
    #[error("expected {expected}, got {actual}")]
    WrongType {
        expected: &'static str,
        actual: String,
        src: String,
    },
    #[error("script exceeded its {limit} limit")]
    LimitExceeded { limit: &'static str, src: String },
}

impl RhaiError {
    pub fn src(&self) -> &str {
        match self {
            RhaiError::Syntax(l) | RhaiError::Runtime(l) => &l.src,
            RhaiError::UnknownName { src, .. }
            | RhaiError::WrongType { src, .. }
            | RhaiError::LimitExceeded { src, .. } => src,
        }
    }

    pub fn from_parse(src: &str, err: ParseError) -> Self {
        RhaiError::Syntax(Located::new(src, err.position(), err.err_type().to_string()))
    }

    pub fn from_eval(src: &str, err: Box<EvalAltResult>) -> Self {
        let src_s = src.to_string();
        match *err {
            EvalAltResult::ErrorTooManyOperations(_) => RhaiError::LimitExceeded {
                limit: "operations",
                src: src_s,
            },
            EvalAltResult::ErrorDataTooLarge(..) => RhaiError::LimitExceeded {
                limit: "size",
                src: src_s,
            },
            EvalAltResult::ErrorStackOverflow(_) => RhaiError::LimitExceeded {
                limit: "call depth",
                src: src_s,
            },
            EvalAltResult::ErrorVariableNotFound(name, _) => RhaiError::UnknownName {
                path: name,
                src: src_s,
            },
            EvalAltResult::ErrorParsing(err_type, pos) => {
                RhaiError::Syntax(Located::new(src, pos, err_type.to_string()))
            }
            other => {
                let pos = other.position();
                RhaiError::Runtime(Located::new(src, pos, other.to_string()))
            }
        }
    }

    pub fn wrong_type(src: &str, expected: &'static str, value: &rhai::Dynamic) -> Self {
        RhaiError::WrongType {
            expected,
            actual: value.type_name().to_string(),
            src: src.to_string(),
        }
    }
}

/// A message anchored to a line/column in the slot source. `Display` prints the
/// message, the offending source line, and a caret under the column.
#[derive(Debug)]
pub struct Located {
    pub src: String,
    /// 1-based; 0 when rhai reported no position.
    pub line: usize,
    /// 1-based; 0 when rhai reported no position.
    pub column: usize,
    pub message: String,
}

impl Located {
    pub fn new(src: &str, pos: Position, message: String) -> Self {
        Located {
            src: src.to_string(),
            line: pos.line().unwrap_or(0),
            column: pos.position().unwrap_or(0),
            message,
        }
    }
}

impl std::fmt::Display for Located {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if self.line == 0 {
            return Ok(());
        }
        let Some(line) = self.src.lines().nth(self.line - 1) else {
            return Ok(());
        };
        write!(f, " (line {}, column {})\n  | {}\n  | ", self.line, self.column, line)?;
        for _ in 1..self.column {
            f.write_str(" ")?;
        }
        f.write_str("^")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_error_shows_line_and_caret() {
        let engine = rhai::Engine::new();
        let src = "let a = 1;\nlet b = a +;";
        let err = engine.compile(src).unwrap_err();
        let e = RhaiError::from_parse(src, err);
        let text = e.to_string();
        assert!(matches!(e, RhaiError::Syntax(_)));
        assert!(text.contains("line 2"), "{text}");
        assert!(text.contains("| let b = a +;"), "{text}");
        assert!(text.lines().last().unwrap().trim_end().ends_with('^'), "{text}");
    }

    #[test]
    fn operations_limit_maps_to_limit_exceeded() {
        let mut engine = rhai::Engine::new();
        engine.set_max_operations(1_000);
        let src = "while true {}";
        let err = engine.run(src).unwrap_err();
        let e = RhaiError::from_eval(src, err);
        assert!(matches!(
            e,
            RhaiError::LimitExceeded { limit: "operations", .. }
        ));
        assert_eq!(e.src(), src);
    }

    #[test]
    fn variable_not_found_maps_to_unknown_name() {
        let engine = rhai::Engine::new();
        let src = "nope + 1";
        let err = engine.eval::<rhai::Dynamic>(src).unwrap_err();
        let e = RhaiError::from_eval(src, err);
        match e {
            RhaiError::UnknownName { path, .. } => assert_eq!(path, "nope"),
            other => panic!("expected UnknownName, got {other:?}"),
        }
    }

    #[test]
    fn runtime_error_carries_source() {
        let engine = rhai::Engine::new();
        let src = "throw \"boom\"";
        let err = engine.run(src).unwrap_err();
        let e = RhaiError::from_eval(src, err);
        assert!(matches!(e, RhaiError::Runtime(_)));
        assert_eq!(e.src(), src);
        assert!(e.to_string().contains("boom"));
    }
}
```

Add to `lib.rs`:

```rust
mod error;
pub use error::{Located, Result, RhaiError};
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p vantage-rhai error`
Expected: 4 passed.

- [ ] **Step 3: Commit**

```bash
git add vantage-rhai
git commit -m "vantage-rhai: RhaiError with located display and limit/unknown-name mapping"
```

---

### Task 3: Limits, Host, Vocab, AST cache

**Files:**
- Create: `vantage-rhai/src/limits.rs`, `vantage-rhai/src/host.rs`
- Modify: `vantage-rhai/src/lib.rs`

**Interfaces:**
- Produces: `Limits::{Ui, Background { max_operations: u64 }}`, `Limits::background() -> Limits` (default ops 50 000 000), `Limits::apply(&self, &mut Engine)`, `trait Vocab { fn register(&self, engine: &mut Engine); }`, `Host::builder(Limits) -> HostBuilder`, `HostBuilder::vocab(self, impl Vocab)`, `HostBuilder::vocab_fn(self, impl FnOnce(&mut Engine))`, `HostBuilder::build(self) -> Host`, `Host::engine(&self) -> &Arc<Engine>`, `Host::ast(&self, key: &str, compile: impl FnOnce(&Engine) -> ParseResult<AST>) -> Result<Arc<AST>>` (cached), `Host::ast_uncached(...)`.
- Later tasks call `Host::ast` with keys `"e:<src>"` (expression) and `"s:<src>"` (script).

- [ ] **Step 1: Write the failing tests**

`vantage-rhai/src/limits.rs`:

```rust
//! Closed set of resource profiles. There is no unlimited engine.

use rhai::Engine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limits {
    /// Anything the UI thread waits on.
    Ui,
    /// Anything under `spawn_blocking`: workers, cmd scripts, imports, faker ticks.
    Background { max_operations: u64 },
}

pub const UI_MAX_OPERATIONS: u64 = 500_000;
pub const BACKGROUND_MAX_OPERATIONS: u64 = 50_000_000;

impl Limits {
    pub fn background() -> Limits {
        Limits::Background {
            max_operations: BACKGROUND_MAX_OPERATIONS,
        }
    }

    pub fn max_operations(&self) -> u64 {
        match self {
            Limits::Ui => UI_MAX_OPERATIONS,
            Limits::Background { max_operations } => *max_operations,
        }
    }

    pub fn apply(&self, engine: &mut Engine) {
        engine.set_max_operations(self.max_operations());
        engine.set_max_string_size(8 * 1024 * 1024);
        engine.set_max_array_size(1_000_000);
        engine.set_max_map_size(100_000);
        engine.set_max_call_levels(64);
        engine.set_max_expr_depths(256, 256);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_profile_bounds_operations() {
        let mut engine = Engine::new();
        Limits::Ui.apply(&mut engine);
        let err = engine.run("while true {}").unwrap_err();
        assert!(matches!(*err, rhai::EvalAltResult::ErrorTooManyOperations(_)));
    }

    #[test]
    fn both_profiles_bound_string_size() {
        for limits in [Limits::Ui, Limits::background()] {
            let mut engine = Engine::new();
            limits.apply(&mut engine);
            let err = engine
                .run(r#"let s = "x"; loop { s += s; }"#)
                .unwrap_err();
            assert!(
                matches!(*err, rhai::EvalAltResult::ErrorDataTooLarge(..)),
                "{limits:?}: {err}"
            );
        }
    }

    #[test]
    fn background_honours_caller_number() {
        let mut engine = Engine::new();
        Limits::Background { max_operations: 10 }.apply(&mut engine);
        assert!(engine.run("let x = 0; for i in 0..100 { x += i; }").is_err());
        let mut engine = Engine::new();
        Limits::background().apply(&mut engine);
        assert!(engine.run("let x = 0; for i in 0..100 { x += i; }").is_ok());
    }
}
```

`vantage-rhai/src/host.rs`:

```rust
//! A configured engine plus a bounded AST cache. Built once per owner, shared.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rhai::{Engine, AST};

use crate::error::{Result, RhaiError};
use crate::limits::Limits;

/// A domain vocabulary: something that registers functions/types on an engine.
pub trait Vocab: Send + Sync {
    fn register(&self, engine: &mut Engine);
}

/// Builds the engine eagerly: limits and the resolver hook go on first, then
/// each vocabulary registers in call order. `vocab_fn` therefore takes a plain
/// `FnOnce` with no `Send`/`'static` bounds, so a caller can move a resolver
/// or a limit into it.
pub struct HostBuilder {
    limits: Limits,
    engine: Engine,
}

impl HostBuilder {
    pub fn vocab(mut self, vocab: impl Vocab) -> Self {
        vocab.register(&mut self.engine);
        self
    }

    pub fn vocab_fn(mut self, f: impl FnOnce(&mut Engine)) -> Self {
        f(&mut self.engine);
        self
    }

    pub fn build(self) -> Host {
        Host {
            engine: Arc::new(self.engine),
            limits: self.limits,
            cache: AstCache::default(),
        }
    }
}

pub const AST_CACHE_BOUND: usize = 1024;

#[derive(Default)]
struct AstCache(Mutex<HashMap<String, Arc<AST>>>);

pub struct Host {
    engine: Arc<Engine>,
    limits: Limits,
    cache: AstCache,
}

impl Host {
    pub fn builder(limits: Limits) -> HostBuilder {
        let mut engine = Engine::new();
        limits.apply(&mut engine);
        crate::resolver::install(&mut engine);
        HostBuilder { limits, engine }
    }

    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Compile through the bounded cache. `key` must encode the compile mode
    /// as well as the source (`"e:<src>"` / `"s:<src>"`) so an expression and a
    /// script with identical text never share an AST.
    pub fn ast(
        &self,
        key: &str,
        src: &str,
        compile: impl FnOnce(&Engine) -> rhai::ParseResult<AST>,
    ) -> Result<Arc<AST>> {
        if let Some(ast) = self.cache.0.lock().unwrap().get(key) {
            return Ok(ast.clone());
        }
        let ast = self.ast_uncached(src, compile)?;
        let mut map = self.cache.0.lock().unwrap();
        if map.len() >= AST_CACHE_BOUND {
            map.clear();
        }
        map.insert(key.to_string(), ast.clone());
        Ok(ast)
    }

    /// Compile without touching the cache (one-off scripts).
    pub fn ast_uncached(
        &self,
        src: &str,
        compile: impl FnOnce(&Engine) -> rhai::ParseResult<AST>,
    ) -> Result<Arc<AST>> {
        compile(&self.engine)
            .map(Arc::new)
            .map_err(|e| RhaiError::from_parse(src, e))
    }

    #[cfg(test)]
    pub(crate) fn cache_len(&self) -> usize {
        self.cache.0.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocab_fn_registers_callable_functions() {
        let host = Host::builder(Limits::Ui)
            .vocab_fn(|e| {
                e.register_fn("double", |x: i64| x * 2);
            })
            .build();
        let ast = host
            .ast("e:double(21)", "double(21)", |e| e.compile_expression("double(21)"))
            .unwrap();
        let v: i64 = host.engine().eval_ast(&ast).unwrap();
        assert_eq!(v, 42);
    }

    #[test]
    fn same_source_compiles_once() {
        let host = Host::builder(Limits::Ui).build();
        let a = host.ast("e:1+1", "1+1", |e| e.compile_expression("1+1")).unwrap();
        let b = host.ast("e:1+1", "1+1", |e| e.compile_expression("1+1")).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(host.cache_len(), 1);
    }

    #[test]
    fn overflow_clears_and_keeps_serving() {
        let host = Host::builder(Limits::Ui).build();
        for i in 0..AST_CACHE_BOUND {
            let src = format!("{i}");
            host.ast(&format!("e:{src}"), &src, |e| e.compile_expression(&src))
                .unwrap();
        }
        assert_eq!(host.cache_len(), AST_CACHE_BOUND);
        host.ast("e:overflow", "1", |e| e.compile_expression("1")).unwrap();
        assert_eq!(host.cache_len(), 1);
        let v: i64 = host
            .engine()
            .eval_ast(&host.ast("e:2", "2", |e| e.compile_expression("2")).unwrap())
            .unwrap();
        assert_eq!(v, 2);
    }

    #[test]
    fn syntax_error_is_located() {
        let host = Host::builder(Limits::Ui).build();
        let err = host
            .ast("e:1 +", "1 +", |e| e.compile_expression("1 +"))
            .unwrap_err();
        assert!(matches!(err, RhaiError::Syntax(_)));
    }
}
```

`build` calls `crate::resolver::install`, which does not exist yet. For this task create a stub `vantage-rhai/src/resolver.rs`:

```rust
//! Lazy variable resolution. Filled in by the resolver task.

use rhai::Engine;

pub(crate) fn install(_engine: &mut Engine) {}
```

Add to `lib.rs`:

```rust
mod host;
mod limits;
mod resolver;
pub use host::{Host, HostBuilder, Vocab, AST_CACHE_BOUND};
pub use limits::{Limits, BACKGROUND_MAX_OPERATIONS, UI_MAX_OPERATIONS};
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p vantage-rhai`
Expected: all pass (previous 6 plus 7 new).

- [ ] **Step 3: Commit**

```bash
git add vantage-rhai
git commit -m "vantage-rhai: Limits profiles, Host builder with Vocab and bounded AST cache"
```

---

### Task 4: Template scanner

**Files:**
- Create: `vantage-rhai/src/template.rs`
- Modify: `vantage-rhai/src/lib.rs`

**Interfaces:**
- Produces: `pub enum Part { Lit(String), Hole(String) }`, `pub fn split(src: &str) -> Result<Vec<Part>>`, `pub fn strip_single_wrapper(src: &str) -> &str`.

- [ ] **Step 1: Write the failing tests**

`vantage-rhai/src/template.rs`:

```rust
//! The one `${ … }` scanner. Brace-nesting aware and string-literal aware.

use crate::error::{Located, Result, RhaiError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Lit(String),
    /// The text between `${` and its matching `}`, untrimmed.
    Hole(String),
}

/// Split a template into literal text and holes.
pub fn split(src: &str) -> Result<Vec<Part>> {
    let mut parts = Vec::new();
    let mut rest = src;
    let mut offset = 0usize;
    while let Some(start) = rest.find("${") {
        if start > 0 {
            parts.push(Part::Lit(rest[..start].to_string()));
        }
        let after = &rest[start + 2..];
        let Some(end) = close_brace(after) else {
            return Err(unterminated(src, offset + start));
        };
        parts.push(Part::Hole(after[..end].to_string()));
        offset += start + 2 + end + 1;
        rest = &after[end + 1..];
    }
    if !rest.is_empty() {
        parts.push(Part::Lit(rest.to_string()));
    }
    Ok(parts)
}

/// `${ expr }` around the whole string → `expr`. Two holes (`${a}${b}`) are
/// not a wrapper and come back unchanged so the caller's compile reports the
/// syntax error.
pub fn strip_single_wrapper(src: &str) -> &str {
    let trimmed = src.trim();
    match trimmed
        .strip_prefix("${")
        .and_then(|r| r.strip_suffix('}'))
    {
        Some(inner) if close_brace(&trimmed[2..]) == Some(trimmed.len() - 3) => inner,
        _ => trimmed,
    }
}

/// Byte index of the `}` that closes a hole whose `${` has already been
/// consumed. Counts nested braces; ignores braces inside `"…"` / `'…'`
/// literals, honouring backslash escapes.
fn close_brace(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 1;
                } else if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'{' => depth += 1,
                b'}' => {
                    if depth == 0 {
                        return Some(i);
                    }
                    depth -= 1;
                }
                _ => {}
            },
        }
        i += 1;
    }
    None
}

fn unterminated(src: &str, at: usize) -> RhaiError {
    let line = src[..at].matches('\n').count() + 1;
    let column = at - src[..at].rfind('\n').map(|p| p + 1).unwrap_or(0) + 1;
    RhaiError::Syntax(Located {
        src: src.to_string(),
        line,
        column,
        message: format!("unterminated `${{` at byte {at}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(s: &str) -> Part {
        Part::Lit(s.to_string())
    }
    fn hole(s: &str) -> Part {
        Part::Hole(s.to_string())
    }

    #[test]
    fn no_holes_is_one_literal() {
        assert_eq!(split("Hello").unwrap(), vec![lit("Hello")]);
        assert_eq!(split("").unwrap(), vec![]);
    }

    #[test]
    fn mixed_text_and_holes() {
        assert_eq!(
            split("Order ${args.id} in ${cur}").unwrap(),
            vec![lit("Order "), hole("args.id"), lit(" in "), hole("cur")]
        );
    }

    #[test]
    fn nested_braces_survive() {
        assert_eq!(
            split(r#"${ if x { "a" } else { "b" } }!"#).unwrap(),
            vec![hole(r#" if x { "a" } else { "b" } "#), lit("!")]
        );
    }

    #[test]
    fn braces_inside_string_literals_are_ignored() {
        assert_eq!(
            split(r#"${ "}" + '{' + "\"}" }"#).unwrap(),
            vec![hole(r#" "}" + '{' + "\"}" "#)]
        );
    }

    #[test]
    fn unterminated_hole_reports_position() {
        let err = split("a\nbb ${x").unwrap_err();
        let RhaiError::Syntax(l) = err else { panic!() };
        assert_eq!((l.line, l.column), (2, 4));
        assert!(l.message.contains("unterminated"));
    }

    #[test]
    fn single_wrapper_is_stripped() {
        assert_eq!(strip_single_wrapper("${ a + b }"), " a + b ");
        assert_eq!(strip_single_wrapper("  a + b  "), "a + b");
        assert_eq!(strip_single_wrapper("${a}${b}"), "${a}${b}");
        assert_eq!(strip_single_wrapper("${ if x { 1 } else { 2 } }"), " if x { 1 } else { 2 } ");
    }
}
```

Add to `lib.rs`:

```rust
pub mod template;
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p vantage-rhai template`
Expected: 6 passed.

- [ ] **Step 3: Commit**

```bash
git add vantage-rhai
git commit -m "vantage-rhai: brace- and string-aware template scanner"
```

---

### Task 5: Resolver, namespace proxy, on_var hook

**Files:**
- Modify: `vantage-rhai/src/resolver.rs` (replace stub), `vantage-rhai/src/lib.rs`

**Interfaces:**
- Produces: `pub enum Lookup { Leaf(Dynamic), Namespace, Unknown }`, `pub trait Resolver: Send + Sync { fn resolve(&self, path: &str) -> Lookup; }`, `pub(crate) const ENV_KEY: &str = "__vantage_env"`, `pub(crate) struct EnvHandle(pub Arc<dyn Resolver>)` (Clone), `pub(crate) struct RecordingResolver { inner, reads: Mutex<BTreeSet<String>> }` with `new(Arc<dyn Resolver>)` and `take_reads(&self) -> BTreeSet<String>`, `pub(crate) fn install(&mut Engine)`.
- Consumes: nothing from other tasks except `Engine`.

- [ ] **Step 1: Write the resolver and its tests**

`vantage-rhai/src/resolver.rs`:

```rust
//! Lazy variable resolution through a `Resolver`, with namespace descent and
//! read recording for discovery.
//!
//! rhai consults `on_var` BEFORE it searches the scope, so the hook yields
//! (`Ok(None)`) for any name the scope already holds: pushed variables shadow
//! the resolver.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use rhai::{Dynamic, Engine, EvalAltResult, Position};

pub enum Lookup {
    Leaf(Dynamic),
    Namespace,
    Unknown,
}

pub trait Resolver: Send + Sync {
    /// Resolve a dotted path. `Namespace` means "keep descending".
    fn resolve(&self, path: &str) -> Lookup;
}

pub(crate) const ENV_KEY: &str = "__vantage_env";

/// The per-evaluation resolver, smuggled into the scope as a constant so a
/// shared engine can serve a different resolver on every call.
#[derive(Clone)]
pub(crate) struct EnvHandle(pub Arc<dyn Resolver>);

/// Wraps a resolver and records every leaf path it resolves.
pub(crate) struct RecordingResolver {
    inner: Arc<dyn Resolver>,
    reads: Mutex<BTreeSet<String>>,
}

impl RecordingResolver {
    pub fn new(inner: Arc<dyn Resolver>) -> Self {
        RecordingResolver {
            inner,
            reads: Mutex::new(BTreeSet::new()),
        }
    }

    pub fn take_reads(&self) -> BTreeSet<String> {
        std::mem::take(&mut *self.reads.lock().unwrap())
    }
}

impl Resolver for RecordingResolver {
    fn resolve(&self, path: &str) -> Lookup {
        let out = self.inner.resolve(path);
        if matches!(out, Lookup::Leaf(_)) {
            self.reads.lock().unwrap().insert(path.to_string());
        }
        out
    }
}

/// A namespace prefix. Indexing (or `.prop`, which rhai routes to the indexer
/// when no getter exists) descends and resolves leaves through the resolver.
#[derive(Clone)]
struct NamespaceProxy {
    resolver: Arc<dyn Resolver>,
    prefix: String,
}

fn lookup(resolver: &Arc<dyn Resolver>, path: &str) -> Result<Dynamic, Box<EvalAltResult>> {
    match resolver.resolve(path) {
        Lookup::Leaf(v) => Ok(v),
        Lookup::Namespace => Ok(Dynamic::from(NamespaceProxy {
            resolver: resolver.clone(),
            prefix: path.to_string(),
        })),
        Lookup::Unknown => Err(Box::new(EvalAltResult::ErrorVariableNotFound(
            path.to_string(),
            Position::NONE,
        ))),
    }
}

fn proxy_index(proxy: &mut NamespaceProxy, key: &str) -> Result<Dynamic, Box<EvalAltResult>> {
    let path = format!("{}.{}", proxy.prefix, key);
    lookup(&proxy.resolver, &path)
}

pub(crate) fn install(engine: &mut Engine) {
    engine.register_type_with_name::<NamespaceProxy>("Namespace");
    engine.register_indexer_get(proxy_index);
    // `on_var` is rhai's variable-resolver hook (flagged "volatile", not truly
    // deprecated — see the rhai docs).
    #[allow(deprecated)]
    engine.on_var(|name, _index, ctx| {
        if name == ENV_KEY || ctx.scope().contains(name) {
            return Ok(None);
        }
        let Some(handle) = ctx.scope().get_value::<EnvHandle>(ENV_KEY) else {
            return Ok(None);
        };
        match handle.0.resolve(name) {
            Lookup::Unknown => Ok(None), // let rhai report it
            _ => lookup(&handle.0, name).map(Some),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A resolver over a flat `path → value` map; any strict prefix of a known
    /// path is a namespace.
    pub(crate) struct MapResolver(pub HashMap<String, Dynamic>);

    impl Resolver for MapResolver {
        fn resolve(&self, path: &str) -> Lookup {
            if let Some(v) = self.0.get(path) {
                return Lookup::Leaf(v.clone());
            }
            let prefix = format!("{path}.");
            if self.0.keys().any(|k| k.starts_with(&prefix)) {
                Lookup::Namespace
            } else {
                Lookup::Unknown
            }
        }
    }

    fn demo() -> Arc<dyn Resolver> {
        let mut m = HashMap::new();
        m.insert("app.toolbar.currency".to_string(), Dynamic::from("USD"));
        m.insert("args.id".to_string(), Dynamic::from(42_i64));
        Arc::new(MapResolver(m))
    }

    fn engine_with(resolver: Arc<dyn Resolver>) -> (Engine, rhai::Scope<'static>) {
        let mut engine = Engine::new();
        install(&mut engine);
        let mut scope = rhai::Scope::new();
        scope.push_constant(ENV_KEY, EnvHandle(resolver));
        (engine, scope)
    }

    #[test]
    fn dotted_reads_descend_namespaces() {
        let (engine, mut scope) = engine_with(demo());
        let v: String = engine
            .eval_with_scope(&mut scope, "app.toolbar.currency")
            .unwrap();
        assert_eq!(v, "USD");
        let n: i64 = engine.eval_with_scope(&mut scope, "args.id + 1").unwrap();
        assert_eq!(n, 43);
    }

    #[test]
    fn pushed_variables_shadow_the_resolver() {
        let (engine, mut scope) = engine_with(demo());
        scope.push("args", 7_i64);
        let n: i64 = engine.eval_with_scope(&mut scope, "args").unwrap();
        assert_eq!(n, 7);
    }

    #[test]
    fn unknown_leaf_is_variable_not_found() {
        let (engine, mut scope) = engine_with(demo());
        let err = engine
            .eval_with_scope::<Dynamic>(&mut scope, "app.toolbar.nope")
            .unwrap_err();
        assert!(matches!(*err, EvalAltResult::ErrorVariableNotFound(ref p, _) if p == "app.toolbar.nope"));
        let err = engine
            .eval_with_scope::<Dynamic>(&mut scope, "nothing")
            .unwrap_err();
        assert!(matches!(*err, EvalAltResult::ErrorVariableNotFound(..)));
    }

    #[test]
    fn recording_resolver_records_leaf_paths_only() {
        let rec = Arc::new(RecordingResolver::new(demo()));
        let as_dyn: Arc<dyn Resolver> = rec.clone();
        let (engine, mut scope) = engine_with(as_dyn);
        engine
            .eval_with_scope::<Dynamic>(&mut scope, "app.toolbar.currency + args.id.to_string()")
            .unwrap();
        let reads = rec.take_reads();
        assert_eq!(
            reads.iter().collect::<Vec<_>>(),
            vec!["app.toolbar.currency", "args.id"]
        );
    }
}
```

Add to `lib.rs`:

```rust
pub use resolver::{Lookup, Resolver};
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p vantage-rhai resolver`
Expected: 4 passed. If `register_indexer_get(proxy_index)` fails type inference, annotate: `engine.register_indexer_get::<NamespaceProxy, &str, false, Dynamic, true>(proxy_index);` is NOT the fix (the const generics are inferred from the closure); instead write it as a closure: `engine.register_indexer_get(|p: &mut NamespaceProxy, key: &str| proxy_index(p, key));`.

- [ ] **Step 3: Commit**

```bash
git add vantage-rhai
git commit -m "vantage-rhai: Resolver trait, namespace proxy and on_var hook with read recording"
```

---

### Task 6: Env and Compiled<S>

**Files:**
- Create: `vantage-rhai/src/compiled.rs`
- Modify: `vantage-rhai/src/lib.rs`, `vantage-rhai/src/slot.rs` (Slot impls), `vantage-rhai/src/resolver.rs` (make `MapResolver` available to crate tests)

**Interfaces:**
- Produces:
  - `Env::new()`, `Env::var(self, name, impl Into<Dynamic>) -> Self`, `Env::resolver(self, Arc<dyn Resolver>) -> Self`, `Env: Default + Clone`.
  - `Host::compile<S: Slot>(&self, &S) -> Result<Compiled<S>>`, `Host::compile_uncached`.
  - `Compiled<S>`: `src()`, `read_set() -> &BTreeSet<String>`, `is_discovered()`, `discover(self, &Env) -> Result<Self>`, `discover_value(self, &Env) -> Result<(Self, Dynamic)>`.
  - `Compiled<Expr>` and `Compiled<Template>`: `eval(&Env) -> Result<Dynamic>`, `eval_bool`, `eval_as::<T: Clone + 'static>`.
  - `Compiled<Template>`: `is_literal()`.
  - `Compiled<Block>`: `run(&Env) -> Result<()>`, `eval(&Env) -> Result<Dynamic>` (final expression value, unit if none).
  - `pub trait Slot` (sealed) implemented by the three slot types.

- [ ] **Step 1: Write the compiled module and tests**

`vantage-rhai/src/compiled.rs`:

```rust
//! Compiled slots and the per-evaluation environment.

use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::sync::Arc;

use rhai::{Dynamic, Engine, AST};

use crate::error::{Result, RhaiError};
use crate::host::Host;
use crate::resolver::{EnvHandle, RecordingResolver, Resolver, ENV_KEY};
use crate::slot::{Block, Expr, Template};
use crate::template::{self, Part};

/// Per-evaluation inputs: pushed variables plus an optional lazy resolver.
#[derive(Default, Clone)]
pub struct Env {
    vars: Vec<(String, Dynamic)>,
    resolver: Option<Arc<dyn Resolver>>,
}

impl Env {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn var(mut self, name: impl Into<String>, value: impl Into<Dynamic>) -> Self {
        self.vars.push((name.into(), value.into()));
        self
    }

    pub fn resolver(mut self, resolver: Arc<dyn Resolver>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    fn scope(&self, resolver: Option<Arc<dyn Resolver>>) -> rhai::Scope<'static> {
        let mut scope = rhai::Scope::new();
        for (name, value) in &self.vars {
            scope.push_dynamic(name.clone(), value.clone());
        }
        if let Some(r) = resolver {
            scope.push_constant(ENV_KEY, EnvHandle(r));
        }
        scope
    }
}

mod sealed {
    pub trait Sealed {}
}

/// What a slot kind knows: how to compile itself into pieces.
pub trait Slot: sealed::Sealed + Sized {
    #[doc(hidden)]
    fn pieces(&self, host: &Host, cached: bool) -> Result<Pieces>;
    #[doc(hidden)]
    fn src(&self) -> &str;
}

#[doc(hidden)]
pub enum Pieces {
    One(Arc<AST>),
    Parts(Vec<TPart>),
}

#[doc(hidden)]
pub enum TPart {
    Lit(String),
    Hole(Arc<AST>),
}

fn expr_ast(host: &Host, src: &str, cached: bool) -> Result<Arc<AST>> {
    let compile = |e: &Engine| e.compile_expression(src);
    if cached {
        host.ast(&format!("e:{src}"), src, compile)
    } else {
        host.ast_uncached(src, compile)
    }
}

fn script_ast(host: &Host, src: &str, cached: bool) -> Result<Arc<AST>> {
    let compile = |e: &Engine| e.compile(src);
    if cached {
        host.ast(&format!("s:{src}"), src, compile)
    } else {
        host.ast_uncached(src, compile)
    }
}

impl sealed::Sealed for Expr {}
impl Slot for Expr {
    fn pieces(&self, host: &Host, cached: bool) -> Result<Pieces> {
        let inner = template::strip_single_wrapper(self.src());
        Ok(Pieces::One(expr_ast(host, inner, cached)?))
    }
    fn src(&self) -> &str {
        Expr::src(self)
    }
}

impl sealed::Sealed for Block {}
impl Slot for Block {
    fn pieces(&self, host: &Host, cached: bool) -> Result<Pieces> {
        Ok(Pieces::One(script_ast(host, self.src(), cached)?))
    }
    fn src(&self) -> &str {
        Block::src(self)
    }
}

impl sealed::Sealed for Template {}
impl Slot for Template {
    fn pieces(&self, host: &Host, cached: bool) -> Result<Pieces> {
        let mut parts = Vec::new();
        for part in template::split(self.src())? {
            parts.push(match part {
                Part::Lit(s) => TPart::Lit(s),
                Part::Hole(h) => TPart::Hole(expr_ast(host, h.trim(), cached)?),
            });
        }
        Ok(Pieces::Parts(parts))
    }
    fn src(&self) -> &str {
        Template::src(self)
    }
}

/// A compiled slot bound to the host's engine. Evaluate many times.
pub struct Compiled<S: Slot> {
    engine: Arc<Engine>,
    src: String,
    pieces: Pieces,
    read_set: BTreeSet<String>,
    discovered: bool,
    _kind: PhantomData<S>,
}

impl Host {
    pub fn compile<S: Slot>(&self, slot: &S) -> Result<Compiled<S>> {
        self.compile_inner(slot, true)
    }

    /// Bypass the AST cache (one-off scripts).
    pub fn compile_uncached<S: Slot>(&self, slot: &S) -> Result<Compiled<S>> {
        self.compile_inner(slot, false)
    }

    fn compile_inner<S: Slot>(&self, slot: &S, cached: bool) -> Result<Compiled<S>> {
        Ok(Compiled {
            engine: self.engine().clone(),
            src: slot.src().to_string(),
            pieces: slot.pieces(self, cached)?,
            read_set: BTreeSet::new(),
            discovered: false,
            _kind: PhantomData,
        })
    }
}

fn render(value: &Dynamic) -> String {
    if value.is_unit() {
        String::new()
    } else {
        value.to_string()
    }
}

impl<S: Slot> Compiled<S> {
    pub fn src(&self) -> &str {
        &self.src
    }

    /// Dotted paths read through the resolver during discovery. Empty until
    /// `discover` has run.
    pub fn read_set(&self) -> &BTreeSet<String> {
        &self.read_set
    }

    pub fn is_discovered(&self) -> bool {
        self.discovered
    }

    /// Evaluate once with reads recorded; freeze the read-set. An unknown name
    /// fails here, before anything renders.
    pub fn discover(self, env: &Env) -> Result<Self> {
        self.discover_value(env).map(|(c, _)| c)
    }

    /// Like `discover`, also returning that first evaluation's value.
    pub fn discover_value(mut self, env: &Env) -> Result<(Self, Dynamic)> {
        let recorder = env
            .resolver
            .clone()
            .map(|r| Arc::new(RecordingResolver::new(r)));
        let as_resolver: Option<Arc<dyn Resolver>> =
            recorder.clone().map(|r| r as Arc<dyn Resolver>);
        let value = self.eval_with(env, as_resolver)?;
        self.read_set = recorder.map(|r| r.take_reads()).unwrap_or_default();
        self.discovered = true;
        Ok((self, value))
    }

    fn eval_ast(&self, ast: &AST, env: &Env, resolver: Option<Arc<dyn Resolver>>) -> Result<Dynamic> {
        let mut scope = env.scope(resolver);
        self.engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, ast)
            .map_err(|e| RhaiError::from_eval(&self.src, e))
    }

    fn eval_with(&self, env: &Env, resolver: Option<Arc<dyn Resolver>>) -> Result<Dynamic> {
        match &self.pieces {
            Pieces::One(ast) => self.eval_ast(ast, env, resolver),
            Pieces::Parts(parts) => {
                if let [TPart::Hole(ast)] = parts.as_slice() {
                    return self.eval_ast(ast, env, resolver);
                }
                let mut out = String::new();
                for part in parts {
                    match part {
                        TPart::Lit(s) => out.push_str(s),
                        TPart::Hole(ast) => {
                            out.push_str(&render(&self.eval_ast(ast, env, resolver.clone())?))
                        }
                    }
                }
                Ok(Dynamic::from(out))
            }
        }
    }

    fn eval_dynamic(&self, env: &Env) -> Result<Dynamic> {
        self.eval_with(env, env.resolver.clone())
    }
}

macro_rules! value_methods {
    ($kind:ty) => {
        impl Compiled<$kind> {
            pub fn eval(&self, env: &Env) -> Result<Dynamic> {
                self.eval_dynamic(env)
            }

            pub fn eval_bool(&self, env: &Env) -> Result<bool> {
                let v = self.eval_dynamic(env)?;
                v.as_bool()
                    .map_err(|_| RhaiError::wrong_type(&self.src, "bool", &v))
            }

            pub fn eval_as<T: Clone + Send + Sync + 'static>(&self, env: &Env) -> Result<T> {
                let v = self.eval_dynamic(env)?;
                v.clone().try_cast::<T>().ok_or_else(|| {
                    RhaiError::wrong_type(&self.src, std::any::type_name::<T>(), &v)
                })
            }
        }
    };
}

value_methods!(Expr);
value_methods!(Template);

impl Compiled<Template> {
    /// No holes: a static value.
    pub fn is_literal(&self) -> bool {
        match &self.pieces {
            Pieces::Parts(parts) => parts.iter().all(|p| matches!(p, TPart::Lit(_))),
            Pieces::One(_) => false,
        }
    }
}

impl Compiled<Block> {
    pub fn run(&self, env: &Env) -> Result<()> {
        self.eval_dynamic(env).map(|_| ())
    }

    /// The script's final expression value, unit when there is none.
    pub fn eval(&self, env: &Env) -> Result<Dynamic> {
        self.eval_dynamic(env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;
    use crate::resolver::tests::MapResolver;
    use std::collections::HashMap;

    fn host() -> Host {
        Host::builder(Limits::Ui).build()
    }

    fn resolver() -> Arc<dyn Resolver> {
        let mut m = HashMap::new();
        m.insert("app.currency".to_string(), Dynamic::from("USD"));
        m.insert("app.show".to_string(), Dynamic::from(true));
        m.insert("args.id".to_string(), Dynamic::from(42_i64));
        Arc::new(MapResolver(m))
    }

    #[test]
    fn expr_evaluates_and_strips_one_wrapper() {
        let h = host();
        let env = Env::new().var("x", 2_i64);
        let c = h.compile(&Expr::from("${ x * 21 }")).unwrap();
        assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 42);
        assert!(h.compile(&Expr::from("${x}${x}")).is_err(), "two holes are not a wrapper");
    }

    #[test]
    fn eval_bool_rejects_non_bool() {
        let h = host();
        let env = Env::new();
        let c = h.compile(&Expr::from("1 + 1")).unwrap();
        assert!(matches!(c.eval_bool(&env), Err(RhaiError::WrongType { expected: "bool", .. })));
        let c = h.compile(&Expr::from("1 < 2")).unwrap();
        assert!(c.eval_bool(&env).unwrap());
    }

    #[test]
    fn expr_rejects_statements() {
        let h = host();
        assert!(matches!(
            h.compile(&Expr::from("let a = 1; a")),
            Err(RhaiError::Syntax(_))
        ));
    }

    #[test]
    fn template_literal_single_and_mixed() {
        let h = host();
        let env = Env::new().var("n", 7_i64).var("u", ());
        let lit = h.compile(&Template::from("plain")).unwrap();
        assert!(lit.is_literal());
        assert_eq!(lit.eval(&env).unwrap().to_string(), "plain");

        let single = h.compile(&Template::from("${ n }")).unwrap();
        assert!(!single.is_literal());
        assert_eq!(single.eval(&env).unwrap().as_int().unwrap(), 7);

        let mixed = h.compile(&Template::from("n=${n} u=[${u}]")).unwrap();
        assert_eq!(mixed.eval(&env).unwrap().to_string(), "n=7 u=[]");
    }

    #[test]
    fn block_runs_and_exposes_final_value() {
        let h = host();
        let env = Env::new();
        let c = h.compile(&Block::from("let a = 40; a + 2")).unwrap();
        c.run(&env).unwrap();
        assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 42);
        let c = h.compile(&Block::from("let a = 1;")).unwrap();
        assert!(c.eval(&env).unwrap().is_unit());
    }

    #[test]
    fn discovery_records_leaf_paths_and_freezes() {
        let h = host();
        let env = Env::new().resolver(resolver());
        let c = h.compile(&Expr::from("app.currency")).unwrap();
        assert!(!c.is_discovered());
        assert!(c.read_set().is_empty());
        let c = c.discover(&env).unwrap();
        assert!(c.is_discovered());
        assert_eq!(c.read_set().iter().collect::<Vec<_>>(), vec!["app.currency"]);
        assert_eq!(c.eval(&env).unwrap().to_string(), "USD");
    }

    #[test]
    fn template_discovery_unions_holes() {
        let h = host();
        let env = Env::new().resolver(resolver());
        let c = h
            .compile(&Template::from("Order ${args.id} in ${app.currency}"))
            .unwrap()
            .discover(&env)
            .unwrap();
        assert_eq!(c.read_set().len(), 2);
        assert_eq!(c.eval(&env).unwrap().to_string(), "Order 42 in USD");
    }

    #[test]
    fn unknown_name_fails_at_discover_not_compile() {
        let h = host();
        let env = Env::new().resolver(resolver());
        let c = h.compile(&Expr::from("app.currencyy")).unwrap();
        match c.discover(&env) {
            Err(RhaiError::UnknownName { path, .. }) => assert_eq!(path, "app.currencyy"),
            other => panic!("{:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn pushed_vars_shadow_resolver_in_env() {
        let h = host();
        let env = Env::new().resolver(resolver()).var("args", 1_i64);
        let c = h.compile(&Expr::from("args")).unwrap().discover(&env).unwrap();
        assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 1);
        assert!(c.read_set().is_empty());
    }

    #[test]
    fn discover_value_returns_the_first_result() {
        let h = host();
        let env = Env::new().resolver(resolver());
        let (c, v) = h
            .compile(&Block::from("let x = args.id; x * 2"))
            .unwrap()
            .discover_value(&env)
            .unwrap();
        assert_eq!(v.as_int().unwrap(), 84);
        assert_eq!(c.read_set().iter().collect::<Vec<_>>(), vec!["args.id"]);
    }

    #[test]
    fn limit_error_surfaces_as_limit_exceeded() {
        let h = host();
        let c = h.compile(&Block::from("while true {}")).unwrap();
        assert!(matches!(
            c.run(&Env::new()),
            Err(RhaiError::LimitExceeded { limit: "operations", .. })
        ));
    }

    #[test]
    fn host_is_shared_across_threads() {
        let h = Arc::new(host());
        let c = Arc::new(h.compile(&Expr::from("x * 2")).unwrap());
        let handles: Vec<_> = (1..=4_i64)
            .map(|i| {
                let c = c.clone();
                std::thread::spawn(move || {
                    c.eval(&Env::new().var("x", i)).unwrap().as_int().unwrap()
                })
            })
            .collect();
        let got: Vec<i64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(got, vec![2, 4, 6, 8]);
    }
}
```

In `resolver.rs`, change `mod tests` to `pub(crate) mod tests` so `MapResolver` is reachable from `compiled.rs` tests.

Add to `lib.rs`:

```rust
mod compiled;
pub use compiled::{Compiled, Env, Slot};
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p vantage-rhai`
Expected: all pass, 12 new in `compiled`.

- [ ] **Step 3: Commit**

```bash
git add vantage-rhai
git commit -m "vantage-rhai: Env and Compiled<Expr|Template|Block> with discovery"
```

---

### Task 7: JSON conversion

**Files:**
- Create: `vantage-rhai/src/json.rs`
- Modify: `vantage-rhai/src/lib.rs`

**Interfaces:**
- Produces: `pub fn to_json(&Dynamic) -> serde_json::Value`, `pub fn from_json(&serde_json::Value) -> Dynamic`.

- [ ] **Step 1: Write the module and test**

`vantage-rhai/src/json.rs`:

```rust
//! `Dynamic` ⇄ `serde_json::Value` on rhai's serde support. Replaces the
//! hand-rolled converters in consumers.

use rhai::Dynamic;
use serde_json::Value;

/// Values serde cannot represent (custom types, function pointers) render as
/// their `to_string()`.
pub fn to_json(value: &Dynamic) -> Value {
    rhai::serde::from_dynamic::<Value>(value).unwrap_or_else(|_| Value::String(value.to_string()))
}

pub fn from_json(value: &Value) -> Dynamic {
    rhai::serde::to_dynamic(value).unwrap_or(Dynamic::UNIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_nested_values() {
        let v = json!({"a": [1, 2.5, true, null, "s"], "b": {"c": -3}});
        let d = from_json(&v);
        assert_eq!(to_json(&d), v);
    }

    #[test]
    fn custom_types_fall_back_to_display() {
        #[derive(Clone)]
        struct Opaque;
        let d = Dynamic::from(Opaque);
        assert!(matches!(to_json(&d), Value::String(_)));
    }
}
```

Add to `lib.rs`:

```rust
mod json;
pub use json::{from_json, to_json};
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p vantage-rhai json`
Expected: 2 passed.

- [ ] **Step 3: Commit**

```bash
git add vantage-rhai
git commit -m "vantage-rhai: to_json/from_json on rhai serde"
```

---

### Task 8: Crate docs, lint, changelog

**Files:**
- Modify: `vantage-rhai/src/lib.rs`, `CHANGELOG.md`

**Interfaces:**
- Produces: the final public surface of `vantage-rhai 0.6.0`: `rhai`, `Expr`, `Template`, `Block`, `Slot`, `Compiled`, `Env`, `Host`, `HostBuilder`, `Vocab`, `Limits`, `Resolver`, `Lookup`, `RhaiError`, `Located`, `Result`, `to_json`, `from_json`, `template::{split, Part}`, `AST_CACHE_BOUND`, `UI_MAX_OPERATIONS`, `BACKGROUND_MAX_OPERATIONS`.

- [ ] **Step 1: Replace the crate doc comment in `lib.rs`**

```rust
//! One Rhai host for every Vantage YAML script slot.
//!
//! A YAML struct holds an [`Expr`], [`Template`] or [`Block`] (source only).
//! A [`Host`] — one `rhai::Engine` with closed [`Limits`] plus a bounded AST
//! cache — compiles it into a [`Compiled`] slot, which evaluates against an
//! [`Env`]: pushed variables plus an optional lazy [`Resolver`].
//!
//! ```ignore
//! let host = Host::builder(Limits::Ui).vocab(MyVerbs).build();
//! let when = host.compile(&Expr::from("row.status == \"placed\""))?;
//! let open = when.eval_bool(&Env::new().var("row", row))?;
//! ```
//!
//! Discovery (`Compiled::discover`) evaluates once with every resolver read
//! recorded and freezes the read-set, which is how framework pages learn what
//! an expression depends on. Consumers use the re-exported [`rhai`] so every
//! crate shares one version and feature set.
```

Keep the existing `mod`/`pub use` lines below it. Confirm the full list:

```rust
pub use rhai;

mod compiled;
mod error;
mod host;
mod json;
mod limits;
mod resolver;
mod slot;
pub mod template;

pub use compiled::{Compiled, Env, Slot};
pub use error::{Located, Result, RhaiError};
pub use host::{Host, HostBuilder, Vocab, AST_CACHE_BOUND};
pub use json::{from_json, to_json};
pub use limits::{Limits, BACKGROUND_MAX_OPERATIONS, UI_MAX_OPERATIONS};
pub use resolver::{Lookup, Resolver};
pub use slot::{Block, Expr, Template};
```

- [ ] **Step 2: Format, lint, and run everything**

Run:

```bash
cd /Users/rw/Work/vantage
cargo fmt -p vantage-rhai
cargo clippy -p vantage-rhai --all-features --all-targets -- -D warnings
cargo test -p vantage-rhai --all-features
```

Expected: no clippy warnings; every test passes. Fix whatever clippy raises in place (typical: `needless_return`, `new_without_default` on `Env` — `Env` already derives `Default`, so add `#[allow(clippy::new_without_default)]` only if it still complains).

- [ ] **Step 3: Add the changelog entry**

Insert at the top of `CHANGELOG.md`, directly under the `All notable changes…` line:

```markdown
## vantage-rhai 0.6.0 - 2026-09-04

- New crate: one Rhai host for every YAML script slot. `Expr` / `Template` /
  `Block` slot types, `Host` with closed `Limits` (no unlimited engine),
  `Vocab` and `Resolver` traits, host-owned read-set discovery, one
  brace- and string-aware `${…}` scanner, bounded AST cache, located errors,
  `to_json` / `from_json`. Design: `plans/2026-09-04-vantage-rhai-host-design.md`.
```

- [ ] **Step 4: Commit**

```bash
git add vantage-rhai CHANGELOG.md
git commit -m "vantage-rhai: crate docs, clippy clean, changelog"
```

---

### Task 9: ui-scope adopts vantage-rhai (vantage-ui repo)

**Files:**
- Modify: `/Users/rw/Work/vantage-ui/framework/ui-scope/Cargo.toml`
- Modify: `/Users/rw/Work/vantage-ui/framework/ui-scope/src/lib.rs`
- Modify: `/Users/rw/Work/vantage-ui/framework/ui-scope/src/expr.rs` (everything above `#[cfg(test)]`)
- Modify: `/Users/rw/Work/vantage-ui/framework/ui-scope/src/script.rs` (everything above `#[cfg(test)]`)

**Interfaces:**
- Consumes: `vantage_rhai::{Host, Limits, Env, Compiled, Resolver, Lookup, Expr, Template, Block, rhai}`.
- Produces (unchanged public API for the rest of vantage-ui): `ui_scope::Expr::{compile(src, &Arc<Scope>) -> anyhow::Result<Expr>, eval() -> Result<Value>, read_set() -> &BTreeSet<String>, src()}`, `ui_scope::Template::{compile, expression, is_literal, eval, read_set() -> BTreeSet<String>}`, `ui_scope::discover_eval(src, &Arc<Scope>, impl FnOnce(&mut Engine)) -> Result<(Dynamic, BTreeSet<String>)>`, `ui_scope::ScriptEnv::{new, over_namespace, run}`, `ui_scope::rhai`.
- New: `ui_scope::FrameResolver(Arc<Scope>)`, `ui_scope::frame_env(&Arc<Scope>) -> Env`, `ui_scope::vantage_rhai` re-export.
- Removed: `ui_scope::limit_engine`, `ui_scope::UI_MAX_OPERATIONS` (their one external caller is fixed in Task 10).

- [ ] **Step 1: Run the existing ui-scope tests to record the baseline**

Run: `cd /Users/rw/Work/vantage-ui && cargo test -p ui-scope 2>&1 | tail -5`
Expected: all pass. Note the count; it must be the same after this task.

- [ ] **Step 2: Switch the dependency**

`framework/ui-scope/Cargo.toml` `[dependencies]` becomes:

```toml
[dependencies]
anyhow = "1.0"
# Path dependency until vantage-rhai 0.6.0 is on crates.io (re-pinned in the publish task).
vantage-rhai = { version = "0.6", path = "../../../vantage/vantage-rhai" }
tokio = { version = "1.52", features = ["sync"] }
```

- [ ] **Step 3: Rewrite `lib.rs`**

```rust
//! The scope graph: sources with reactivity classes, lexical scopes,
//! dependency discovery at init, frozen expressions, subscription wiring and
//! the two invalidation classes (value / structural).
//!
//! Rhai plumbing (engine, limits, `${…}` scanner, discovery) lives in
//! `vantage-rhai`; this crate binds it to the scope frame.

mod expr;
mod frame;
mod script;
mod source;
mod value;
mod wiring;

pub use expr::{discover_eval, frame_env, Expr, FrameResolver, Template};
pub use frame::Scope;
// Callers extending the engine (custom verbs) must use the same rhai.
pub use vantage_rhai::{self, rhai};
pub use script::ScriptEnv;
pub use source::{Generation, Reactivity, Source};
pub use value::{dynamic_to_value, value_to_dynamic, Value};
pub use wiring::{Connection, Invalidation, NotifyHook, Wiring};
```

- [ ] **Step 4: Rewrite the non-test part of `expr.rs`**

Replace everything above `#[cfg(test)]` with:

```rust
//! Frozen expressions with discovered read-sets, over `vantage-rhai`.
//!
//! Security note: "eval" here means the embedded Rhai engine evaluating
//! inventory-authored expressions in a sandboxed scope (no filesystem, no
//! process access), under `Limits::Ui`.
//!
//! `${...}` content is a Rhai expression; a plain dotted path is valid Rhai.
//! Compilation runs the host's discovery pass once against the frame: the
//! recorded dotted paths become the expression's frozen read-set.
//! Re-evaluation reuses the compiled AST; the dependency set never changes
//! after init — structural invalidation recompiles instead.

use std::collections::BTreeSet;
use std::sync::{Arc, LazyLock};

use anyhow::{anyhow, Context, Result};
use rhai::{Dynamic, Engine};
use vantage_rhai::{Compiled, Env, Host, Limits, Lookup, Resolver};

use crate::frame::Scope;
use crate::value::{dynamic_to_value, value_to_dynamic, Value};

/// The process-wide host for plain expressions: `Limits::Ui`, no vocabulary.
static UI_HOST: LazyLock<Host> = LazyLock::new(|| Host::builder(Limits::Ui).build());

/// Resolves dotted paths against a scope frame.
pub struct FrameResolver(pub Arc<Scope>);

impl Resolver for FrameResolver {
    fn resolve(&self, path: &str) -> Lookup {
        if let Some(source) = self.0.lookup(path) {
            return Lookup::Leaf(value_to_dynamic(source.get()));
        }
        if self.0.is_namespace(path) {
            Lookup::Namespace
        } else {
            Lookup::Unknown
        }
    }
}

/// An `Env` whose reads resolve through `frame`.
pub fn frame_env(frame: &Arc<Scope>) -> Env {
    Env::new().resolver(Arc::new(FrameResolver(frame.clone())))
}

/// One-shot script evaluation with instrumented reads and caller-supplied
/// engine extensions (custom types, domain verbs — e.g. the form options
/// script registers Vista-building verbs). Accepts full scripts (statements),
/// returns the final value and the recorded read-set; the caller decides what
/// the read-set means.
pub fn discover_eval(
    src: &str,
    frame: &Arc<Scope>,
    customize: impl FnOnce(&mut Engine),
) -> Result<(Dynamic, BTreeSet<String>)> {
    let host = Host::builder(Limits::Ui).vocab_fn(customize).build();
    let env = frame_env(frame);
    let (compiled, value) = host
        .compile_uncached(&vantage_rhai::Block::from(src))
        .and_then(|c| c.discover_value(&env))
        .map_err(|e| anyhow!("script eval failed: {e}"))?;
    Ok((value, compiled.read_set().clone()))
}

/// A compiled Rhai expression with its frozen read-set.
pub struct Expr {
    inner: Compiled<vantage_rhai::Expr>,
    env: Env,
    src: String,
}

impl Expr {
    /// Compile and run the discovery evaluation. Fails on syntax errors and on
    /// reads of unknown names — at init, before anything renders.
    pub fn compile(src: &str, frame: &Arc<Scope>) -> Result<Expr> {
        let env = frame_env(frame);
        let inner = UI_HOST
            .compile(&vantage_rhai::Expr::from(src))
            .with_context(|| format!("cannot compile `{src}`"))?
            .discover(&env)
            .map_err(|e| anyhow!("discovery eval of `{src}` failed: {e}"))?;
        Ok(Expr {
            inner,
            env,
            src: src.to_string(),
        })
    }

    /// Re-evaluate against current source values using the frozen AST.
    pub fn eval(&self) -> Result<Value> {
        let result = self
            .inner
            .eval(&self.env)
            .map_err(|e| anyhow!("eval of `{}` failed: {e}", self.src))?;
        dynamic_to_value(result)
    }

    /// The dotted paths this expression was discovered to read. Frozen at init.
    pub fn read_set(&self) -> &BTreeSet<String> {
        self.inner.read_set()
    }

    pub fn src(&self) -> &str {
        &self.src
    }
}

impl std::fmt::Debug for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Expr")
            .field("src", &self.src)
            .field("read_set", self.read_set())
            .finish()
    }
}

enum Inner {
    Text(Compiled<vantage_rhai::Template>),
    Expr(Compiled<vantage_rhai::Expr>),
}

/// A YAML string value: a plain literal, a single `${...}` expression (typed
/// result), or a mixed text template (string result).
pub struct Template {
    inner: Inner,
    env: Env,
}

impl Template {
    pub fn compile(src: &str, frame: &Arc<Scope>) -> Result<Template> {
        let env = frame_env(frame);
        let inner = UI_HOST
            .compile(&vantage_rhai::Template::from(src))
            .with_context(|| format!("cannot compile `{src}`"))?
            .discover(&env)
            .map_err(|e| anyhow!("discovery eval of `{src}` failed: {e}"))?;
        Ok(Template {
            inner: Inner::Text(inner),
            env,
        })
    }

    /// Compile the WHOLE string as one Rhai expression — the `when:` gate
    /// semantics. A text template's eval renders a STRING, and any non-empty
    /// string is truthy, so `'"${x}" == "y"'` as a gate is silently always
    /// open; expression form (`x == "y"`) evaluates to a real bool. A single
    /// `${...}` wrapper is tolerated (and stripped) for symmetry with reads.
    pub fn expression(src: &str, frame: &Arc<Scope>) -> Result<Template> {
        let env = frame_env(frame);
        let inner = UI_HOST
            .compile(&vantage_rhai::Expr::from(src))
            .with_context(|| format!("cannot compile `{src}`"))?
            .discover(&env)
            .map_err(|e| anyhow!("discovery eval of `{src}` failed: {e}"))?;
        Ok(Template {
            inner: Inner::Expr(inner),
            env,
        })
    }

    /// True when the template contains no `${...}` — a static source (D5).
    pub fn is_literal(&self) -> bool {
        match &self.inner {
            Inner::Text(t) => t.is_literal(),
            Inner::Expr(_) => false,
        }
    }

    pub fn eval(&self) -> Result<Value> {
        let result = match &self.inner {
            Inner::Text(t) => t.eval(&self.env),
            Inner::Expr(e) => e.eval(&self.env),
        }
        .map_err(|e| anyhow!("eval failed: {e}"))?;
        dynamic_to_value(result)
    }

    /// Union of the parts' frozen read-sets. Empty for literals.
    pub fn read_set(&self) -> BTreeSet<String> {
        match &self.inner {
            Inner::Text(t) => t.read_set().clone(),
            Inner::Expr(e) => e.read_set().clone(),
        }
    }
}
```

The test module below stays byte-for-byte as it is.

- [ ] **Step 5: Rewrite the non-test part of `script.rs`**

Replace everything above `#[cfg(test)]` with:

```rust
//! Action scripts — the one narrow write path (spec D10).
//!
//! Scripts evaluate in the same sandboxed Rhai environment as expressions
//! (reads resolve through the scope frame), plus ONE writable root: writes are
//! allowed only to explicitly declared variables (`app.last_product_id = ...`);
//! an undeclared write is an error, not a new variable.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use rhai::{Dynamic, EvalAltResult};
use vantage_rhai::{Block, Env, Host, Limits};

use crate::frame::Scope;
use crate::source::Source;
use crate::value::{dynamic_to_value, value_to_dynamic};

/// The writable `app` root handed to action scripts. Reads fall back to the
/// frame (so `app.toolbar.default_currency` stays readable); writes hit the
/// declared-variables table or fail.
#[derive(Clone)]
struct WriteProxy {
    root: String,
    frame: Arc<Scope>,
    writable: Arc<HashMap<String, Source>>,
    /// Writes may also land on any live `<root>.<name>` the frame
    /// resolves — for a root whose variables are declared elsewhere (a
    /// step's `state:` block seeds the frame; a button's script writes
    /// through it). Still never a new variable.
    frame_writable: bool,
}

fn write_proxy_get(proxy: &mut WriteProxy, key: &str) -> Result<Dynamic, Box<EvalAltResult>> {
    if let Some(source) = proxy.writable.get(key) {
        return Ok(value_to_dynamic(source.get()));
    }
    let path = format!("{}.{}", proxy.root, key);
    if let Some(source) = proxy.frame.lookup(&path) {
        return Ok(value_to_dynamic(source.get()));
    }
    if proxy.frame.is_namespace(&path) {
        // Deeper namespaces are read-only from actions in the prototype.
        return Err(format!("`{path}` is a namespace; actions read leaf values").into());
    }
    Err(format!("unknown name `{path}`").into())
}

fn write_proxy_set(
    proxy: &mut WriteProxy,
    key: &str,
    value: Dynamic,
) -> Result<(), Box<EvalAltResult>> {
    let declared = proxy.writable.get(key).cloned();
    let in_frame = || {
        proxy
            .frame_writable
            .then(|| proxy.frame.lookup(&format!("{}.{key}", proxy.root)))
            .flatten()
    };
    let Some(source) = declared.or_else(in_frame) else {
        return Err(format!(
            "cannot write `{}.{key}`: not a declared variable",
            proxy.root
        )
        .into());
    };
    let value = dynamic_to_value(value).map_err(|e| e.to_string())?;
    source.set(value).map_err(|e| e.to_string())?;
    Ok(())
}

/// The action-script environment: a frame for reads, one writable root.
pub struct ScriptEnv {
    host: Host,
    proxy: WriteProxy,
}

impl ScriptEnv {
    /// `writable` maps bare variable names (`last_product_id`) to their live
    /// sources; they appear to scripts as `<root>.<name>`.
    pub fn new(
        frame: Arc<Scope>,
        root: impl Into<String>,
        writable: HashMap<String, Source>,
    ) -> Self {
        // Runs on the UI thread, so it gets the time bound too.
        let host = Host::builder(Limits::Ui)
            .vocab_fn(|engine| {
                engine.register_type_with_name::<WriteProxy>("WriteProxy");
                engine.register_indexer_get(write_proxy_get);
                engine.register_indexer_set(write_proxy_set);
            })
            .build();
        let proxy = WriteProxy {
            root: root.into(),
            frame,
            writable: Arc::new(writable),
            frame_writable: false,
        };
        Self { host, proxy }
    }

    /// An environment whose writable root is a namespace the frame
    /// already holds: `<root>.<name>` is writable when the frame
    /// resolves it to a live source. The way a page-level script reaches
    /// variables some other owner declared (a step's `state:`).
    pub fn over_namespace(frame: Arc<Scope>, root: impl Into<String>) -> Self {
        let mut env = Self::new(frame, root, HashMap::new());
        env.proxy.frame_writable = true;
        env
    }

    pub fn run(&self, script: &str) -> Result<()> {
        let env = Env::new().var(self.proxy.root.clone(), Dynamic::from(self.proxy.clone()));
        self.host
            .compile(&Block::from(script))
            .and_then(|block| block.run(&env))
            .map_err(|e| anyhow!("action failed: {e}"))
    }
}
```

The test module below stays byte-for-byte as it is.

- [ ] **Step 6: Run the ui-scope tests**

Run: `cargo test -p ui-scope 2>&1 | tail -5`
Expected: same count as Step 1, all pass. If `unknown_name_fails_at_compile` fails, the message must contain the path: check that `RhaiError::UnknownName`'s `Display` is `unknown name \`app.toolbar.default_currencyy\` in scope` and that `discover` (not `compile`) is where it surfaces.

- [ ] **Step 7: Commit (vantage-ui repo)**

```bash
cd /Users/rw/Work/vantage-ui
git checkout -b vantage-rhai-adoption
git add framework/ui-scope Cargo.lock
git commit -m "ui-scope: run on vantage-rhai (Host, Limits::Ui, Resolver over the frame)"
```

---

### Task 10: Fix the one external caller and check the workspace (vantage-ui repo)

**Files:**
- Modify: `/Users/rw/Work/vantage-ui/crates/wizard/src/runtime.rs:362-366`

**Interfaces:**
- Consumes: `ui_scope::vantage_rhai::Limits`.

- [ ] **Step 1: Replace the `limit_engine` call**

Current lines 363-366:

```rust
    // No operation ceiling: a worker legitimately runs for minutes, and
    // `on_progress` below ends it on cancel. The size limits still
    // apply — cancellation bounds time, not memory.
    ui_scope::limit_engine(engine, None);
```

become:

```rust
    // Background profile: a worker legitimately runs for a long time, so
    // the operation budget is the large one, and `on_progress` below ends
    // it on cancel. Size limits apply as everywhere else. The full move of
    // this engine onto a `vantage_rhai::Host` is the site-migration spec.
    ui_scope::vantage_rhai::Limits::background().apply(engine);
```

- [ ] **Step 2: Check the whole workspace and run the affected crates' tests**

Run:

```bash
cd /Users/rw/Work/vantage-ui
cargo check --workspace 2>&1 | tail -3
cargo test -p ui-scope -p vantage-wizard 2>&1 | tail -5
```

Expected: check clean (the `ui_scope::rhai::Engine::new()` in `framework/ui-component/src/components/stat.rs:103` and `use ui_scope::rhai;` in `render.rs:60` keep compiling through the re-export); tests pass.

- [ ] **Step 3: Commit (vantage-ui repo)**

```bash
git add crates/wizard/src/runtime.rs
git commit -m "wizard: background Limits profile from vantage-rhai instead of limit_engine"
```

---

### Task 11: Publish and re-pin

**Files:**
- Modify: `/Users/rw/Work/vantage-ui/framework/ui-scope/Cargo.toml`

- [ ] **Step 1: Publish `vantage-rhai` (manual, needs the user's crates.io token)**

```bash
cd /Users/rw/Work/vantage
cargo publish -p vantage-rhai --dry-run
cargo publish -p vantage-rhai
```

Expected: dry run clean, then `Uploaded vantage-rhai v0.6.0`.

- [ ] **Step 2: Re-pin ui-scope to the published crate**

In `framework/ui-scope/Cargo.toml` replace the path dependency line with:

```toml
vantage-rhai = "0.6.0"
```

Run:

```bash
cd /Users/rw/Work/vantage-ui
cargo update -p vantage-rhai
cargo test -p ui-scope 2>&1 | tail -3
cargo check --workspace 2>&1 | tail -2
```

Expected: resolves from crates.io, tests pass, check clean.

- [ ] **Step 3: Commit (vantage-ui repo)**

```bash
git add framework/ui-scope/Cargo.toml Cargo.lock
git commit -m "ui-scope: depend on published vantage-rhai 0.6.0"
```

- [ ] **Step 4: Push both branches and open PRs**

```bash
cd /Users/rw/Work/vantage && git push -u origin vantage-rhai-host-spec
cd /Users/rw/Work/vantage-ui && git push -u origin vantage-rhai-adoption
```

Open a PR per repo. The vantage PR body should reference `plans/2026-09-04-vantage-rhai-host-design.md`; the vantage-ui PR should link the vantage PR and state that ui-scope's test count is unchanged.

---

## Self-review notes

- Spec §1 slot types → Task 1. §2 host/limits/vocab → Task 3. §3 env/resolver/discovery → Tasks 5, 6. §4 scanner → Task 4. §5 caching → Task 3 (cache) and Task 6 (`Compiled` owns its AST). §6 errors → Task 2. §7 conversion → Task 7. §8 day one → Tasks 8–11. §9 tests: scanner (Task 4), slot kinds (Task 6), limits (Task 3), discovery (Tasks 5, 6), cache (Task 3), sync (Task 6), errors (Task 2), conversion (Task 7).
- Deviation from spec §1 table, deliberate: `Compiled<Block>::eval` returns the script's final value (unit when none) in addition to `run`, because `discover_eval` for form `options:` scripts needs a statement sequence's value. `discover_value` exists on every kind for the same reason.
- `HostBuilder::vocab_fn` takes `FnOnce(&mut Engine)` with no `Send`/`'static` bound (engine built eagerly) so `discover_eval` keeps its current signature.
- rhai calls `on_var` before searching the scope; the hook yields when `ctx.scope().contains(name)`, which is what makes pushed variables shadow the resolver (tested in Tasks 5 and 6).

