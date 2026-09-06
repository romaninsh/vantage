# vantage-rhai

One [Rhai](https://rhai.rs) host for every script slot in Vantage's YAML.

A slot is a YAML field whose value is script source: a `when:` guard, a `text:`
template, an `action:` body, a driver's query builder. Before this crate each
consumer built its own `rhai::Engine`, so limits, error shape, caching and
`${…}` parsing were settled independently by every consumer and agreed on
nothing.

```toml
[dependencies]
vantage-rhai = "0.6"
```

Do not add `rhai` beside it. This crate re-exports the exact version and
feature set it was built against as `vantage_rhai::rhai`. A same-major entry of
your own quietly unions its default features onto the shared build; a different
major gives you two `Dynamic` types that rustc can only explain as a note about
multiple versions of `rhai` hanging off a mismatched-types error.

## The three kinds of slot

Reach for `Expr` unless the author needs surrounding text (`Template`) or more
than one statement (`Block`).

| Type | Author writes | Compiled as |
|---|---|---|
| `Expr` | one expression: `row.total > 100` | a single value |
| `Template` | text with holes: `Due ${ row.days } days` | literal parts joined with evaluated holes |
| `Block` | statements: `let n = count(); save(n);` | a script; its final *expression* is the result, unit if it ends on a statement |

Each is a newtype over `String` with `#[serde(transparent)]`, so a YAML struct
declares the kind and deserializes from a plain scalar:

```rust
use vantage_rhai::{Block, Expr, Template};

#[derive(serde::Deserialize)]
struct ButtonDef {
    when: Option<Expr>,
    label: Template,
    action: Block,
}
```

They deref to `str`, print as their source, serialise back out as the same
scalar, compare against `&str`, hash, and `Default` to empty — so changing a
field from `Option<String>` to `Option<Expr>` leaves every site that only
displays or compares it untouched. The kind bites where it should:
`Host::compile` takes the typed slot, never a bare string.

Two consequences of the kinds being real types:

- An `Expr` tolerates one `${ … }` wrapped around the whole string and strips
  it, so an author who writes `when: '${ row.ok }'` out of habit gets the
  expression they meant rather than a template that renders `"true"` and is
  therefore always truthy.
- An `Expr` refuses statements outright — `when: 'let a = 1; a'` is a syntax
  error, because the kind compiles through rhai's expression parser.

## Compiling and evaluating

```rust
use vantage_rhai::{Env, Expr, Host, Limits};

let host = Host::builder(Limits::Ui).build();
let guard = host.compile(&Expr::from("qty > 0 && qty <= stock"))?;

let env = Env::new().var("qty", 3_i64).var("stock", 10_i64);
assert!(guard.eval_bool(&env)?);
# Ok::<(), vantage_rhai::RhaiError>(())
```

`compile` returns a `Compiled<Expr>` — source, AST and engine in one value —
which you keep and re-evaluate against a fresh `Env` per row, per frame, per
tick.

On a `Compiled<S>`:

- `eval` gives the raw `Dynamic`, `eval_bool` a `bool`, `eval_as::<T>` any
  `Clone + Send + Sync + 'static` type. A mismatch is `RhaiError::WrongType`,
  not a panic.
- `run` on a `Compiled<Block>` discards the value, for a script executed for
  effect.
- `is_literal` on a `Compiled<Template>` reports a template with no holes,
  which lets a caller skip re-evaluating a constant.
- `engine()` and `ast()` hand out the compiled pair for the one thing the API
  cannot express: evaluating a script to a `FnPtr` once and calling that
  closure per line. `ast()` is an `Option` — a template with text around its
  holes has one AST per hole and no single script to hand out.

A template that is **one hole and nothing else** evaluates to that hole's value
with its type intact, so a slot can accept either text or structure:
`'${ #{ text: label, color: "red" } }'` yields the map, and
`Template::from("${ 1 + 1 }").eval_as::<String>()` is a `WrongType` error
rather than `"2"`. Add a single space of literal text and the parts join into a
`String` instead, with a unit-valued hole rendering as empty.

Use `compile` for anything a project holds onto — the host's cache means the
same source parses once. Use `compile_uncached` for a one-shot script that will
never repeat: the cache holds `AST_CACHE_BOUND` entries and clears wholesale
when it overflows, so a throwaway entry costs every other entry, not just
itself. `Mode` distinguishes the two parsers in the cache key, so an `Expr` and
a `Block` with identical text never collide.

`Host` is `Send + Sync` and built to be shared — a `LazyLock` static serving
every thread is the intended shape, not a workaround.

## Env: pushed variables and lazy resolution

`Env` carries what one evaluation can see. Two mechanisms:

```rust
use std::sync::Arc;
use vantage_rhai::{Env, Lookup, Resolver};

let env = Env::new()
    .var("row", Dynamic::from(row_handle))
    .resolver(Arc::new(PageScope(frame)));
```

`var` takes anything `Into<Dynamic>`, which covers numbers, strings and bools.
Your own type goes in as `Dynamic::from(value)` and must be
`Clone + Send + Sync + 'static`; forget the wrapper and rustc reports a missing
`ImmutableString: From<YourType>` without ever mentioning `Dynamic`.

Supply a `Resolver` when the value space is large or expensive — a page scope
with hundreds of entries, of which one expression reads two.

```rust
impl Resolver for PageScope {
    fn resolve(&self, path: &str) -> Lookup {
        match self.0.lookup(path) {
            Some(value) => Lookup::Leaf(value.into()),
            None if self.0.is_namespace(path) => Lookup::Namespace,
            None => Lookup::Unknown,
        }
    }
}
```

`Lookup::Namespace` is how dotted paths work: `orders.selected.id` resolves
`orders`, gets `Namespace`, descends, and only the full path becomes a `Leaf`.
`Unknown` raises `RhaiError::UnknownName`, so a typo fails loudly rather than
evaluating to unit.

Pushed variables shadow the resolver. Rhai consults its variable hook before
searching the scope, and the hook declines any name the scope already holds.

## Limits: there is no unlimited engine

Two profiles:

| Profile | Operation ceiling | For |
|---|---|---|
| `Limits::Ui` | 500,000 | anything the UI thread waits on |
| `Limits::background()` | 50,000,000 | anything under `spawn_blocking` |

Both also cap string size at 8 MiB, arrays at a million elements, maps at
100,000 keys, call nesting at 64 and expression depth at 256.
`Limits::Background` takes a custom ceiling if you need one; it is clamped to
at least 1, because rhai reads a ceiling of zero as *no ceiling* and that would
quietly invert the guarantee.

The point is not to stop an attacker — see the trust model below — but to turn
a mistyped loop into an error the author can read instead of a frozen window.

## Vocabularies

A bare host can do arithmetic and string work and nothing else. Domain verbs
arrive as a `Vocab`:

```rust
use vantage_rhai::rhai::Engine;
use vantage_rhai::{Host, Limits, Vocab};

pub struct InvoiceVocab;

impl Vocab for InvoiceVocab {
    fn register(&self, engine: &mut Engine) {
        engine.register_type_with_name::<Invoice>("Invoice");
        engine.register_fn("total", Invoice::total);
    }
}

let host = Host::builder(Limits::background()).vocab(InvoiceVocab).build();
```

Prefer a unit struct when another crate needs to name the vocabulary; reach for
`.vocab_fn(|engine| …)` when the registration is private to one host. The
closure is `FnOnce` with no `Send` or `'static` bound, so it can move a
resolver or a limit straight in.

**Registration order decides collisions.** Vocabs register in call order and
the last definition of a name wins. Vantage's data layer depends on this: a
vendor vocabulary defines `table` as an identifier constructor and the
conventional vocabulary defines it as a query builder, so registering the
vendor one first is what makes `table("order")` mean the query builder on a
host that has both.

**Never call `engine.on_var` from a vocabulary.** The host installs the
resolver on that hook before any vocab registers, rhai keeps only the last
hook, and a second one therefore disables resolution for every slot on the
host — silently. Push the value through `Env` instead. The SurrealDB backend's
`me` constant was an `on_var` hook until it collided this way.

Registering a *type* and pushing the *instance* through `Env` lets one host
serve every row; baking a specific row into the engine at registration time
forces an engine per row, which is what the grid used to do per predicate per
render.

## Discovery: learning what a script reads

`discover` evaluates once with every resolver read recorded, then freezes the
set:

```rust
let compiled = host.compile(&Expr::from("orders.total * tax.rate"))?
    .discover(&env)?;              // `env` must carry a resolver
assert_eq!(compiled.read_set().len(), 2);
```

This is how a framework page learns its dependencies without parsing the
script: subscribe to exactly the paths in `read_set()`, re-evaluate when one
changes. `discover_value` returns that first value too, so the discovery pass
doubles as the first render.

Only reads through the resolver are recorded — a pushed variable is not a read,
and an `Env` with no resolver discovers nothing. `discover` consumes and
returns the `Compiled`, so it cannot run twice; `is_discovered()` distinguishes
a frozen empty set from one that was never taken. Treat the result as a
dependency floor: a branch the evaluation did not take recorded nothing, so
recompile on structural change.

`cargo run --example invalidation` puts a number on this. Two slots each roll a
random value whenever they run, so you can see which ones a change recomputed:
bumping a path recomputes the slot that reads it and leaves the other holding
its original number, and bumping a sibling path that neither slot reads
recomputes nothing.

## Errors

`RhaiError` has five variants — `Syntax`, `UnknownName`, `Runtime`,
`WrongType`, `LimitExceeded` — and every one carries the slot source via
`src()`.

`Syntax` and `Runtime` additionally carry a `Located`, whose `line`, `column`,
`message` and `src` fields are public so a validator can emit a structured
diagnostic instead of re-parsing rendered text. Its `Display` draws the
offending line with a caret:

```
Syntax error: Expecting ';' to terminate this statement (line 3, column 1)
  | let m = 2;
  | ^
```

For a template, those coordinates are remapped into the whole template rather
than the isolated hole, so an error on line 2 of a block scalar says line 2.

Compiling every slot at load turns this into catalogue validation: collect the
failures, and report each one's YAML key beside `src()` and the `Located`
position. An author then learns about a typo before the page opens rather than
when the row that triggers it renders.

## The `${…}` scanner

`template::split` is the one place `${…}` is parsed. It returns
`Part::Lit`/`Part::Hole` and understands nested braces, `"…"` and `'…'` with
escapes, backtick strings, and `//` and `/* */` comments — so
`${ if n > 1 { "many" } else { "one" } }` is one hole and a brace inside a
string is text.

Call it directly for a template that is *not* Rhai — an environment-variable
expansion, a scope-path check in a validator — so those keep one definition of
where a hole starts and ends while resolving their own way. An unterminated
`${` is `Err`, with the position.

## JSON

`to_json` and `from_json` bridge `Dynamic` and `serde_json::Value`
structurally. A leaf serde cannot represent renders as its `to_string()`, which
costs that leaf rather than collapsing the object around it into one string.

## Cargo features

`schema` adds `JsonSchema` for the three slot types, each emitting
`{"type": "string", "description": …, "x-language": "rhai"}`. Editors read the
marker to inject Rhai highlighting into the YAML string and show the
description as a tooltip, so a project's generated schema advertises every
scripted field for free.

## Shared hosts

`ui_host()` and `background_host()` are process-wide, vocabulary-free hosts on
the two profiles. Take one when your slot needs no verbs — a projection
formula, a list row's template, an action argument — and it shares one AST
cache with every other such caller. Build your own the moment you need a verb;
these have none deliberately, so nothing comes to depend on a vocabulary
someone else registered.

## Trust model

Scripts here are written by the same people who write the project's YAML, and
carry the same trust as the YAML itself. This is not a sandbox for third-party
code. Rhai reaches nothing on its own — no filesystem, no network, no
processes — so a host's attack surface is exactly what its vocabulary
registers. Keep that in mind when a verb wraps something dangerous: a `run()`
that spawns a command should capture the command at registration time rather
than take it as an argument, which is how `vantage-cmd` keeps a script from
choosing its own binary.

## Adding a scripted slot to a crate

1. Depend on `vantage-rhai`, not `rhai`. Enable `schema` if you generate JSON
   schemas for your YAML.
2. Type the YAML field `Expr`, `Template` or `Block`.
3. Decide the profile: does the caller block the UI thread, or is this under
   `spawn_blocking`?
4. Write a `Vocab` for your verbs, registering types rather than instances.
5. Build the host once and compile each slot through it:

   ```rust
   fn host() -> &'static Host {
       static HOST: LazyLock<Host> =
           LazyLock::new(|| Host::builder(Limits::Ui).vocab(InvoiceVocab).build());
       &HOST
   }
   ```

6. Evaluate against a fresh `Env` per call, pushing the per-call values as
   `Dynamic::from(…)`.
7. Surface `RhaiError` with its `src()` so the author sees which slot failed.

## Examples

Both load their slots from a YAML file beside them, the way a project does.

| Example | Shows |
|---|---|
| `cargo run --example quickstart` | the seven steps above end to end: all three slot kinds, a vocabulary, a resolver, discovery, and what each error looks like |
| `cargo run --example invalidation` | how a consumer turns a read-set into a decision about what to recompute |
| `cargo run --example two_hosts` | two hosts on two threads sharing a scope: per-path generations, a pump loop, and a label that falls quiet when its value stops moving |

## Layout

| File | Contents |
|---|---|
| `slot.rs` | `Expr`, `Template`, `Block` and their schema impls |
| `host.rs` | `Host`, `HostBuilder`, `Vocab`, the AST cache, shared hosts |
| `limits.rs` | the two profiles and what they cap |
| `compiled.rs` | `Compiled<S>`, `Env`, the `Slot` trait, discovery |
| `resolver.rs` | `Resolver`, `Lookup`, namespace descent, read recording |
| `template.rs` | the `${…}` scanner |
| `error.rs` | `RhaiError` and located display |
| `json.rs` | `Dynamic` ⇄ `serde_json::Value` |
