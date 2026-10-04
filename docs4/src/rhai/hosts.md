# Hosts

A script never runs on a bare Rhai engine. It runs in a [`Host`](vantage_rhai::Host) from
`vantage-rhai`: one engine with fixed limits, a bounded cache of compiled scripts, and the
vocabularies registered on it. This chapter is for the Rust side: how an application builds a host
for the data vocabulary and what each setting decides.

<!-- toc -->

---

## Host, Vocab, Env

A host is built once and reused. Compiling through it is cached, so evaluating the same script on
every row costs one parse:

<!-- tested: rhai_guide::hosts::build_a_host -->
```rust,ignore
let host = Host::builder(Limits::background())
    .vocab(DataVocab::read_write_with(
        Some(resolver(&store)),
        Some(50),
        Writes::Allowed,
    ))
    .build();

let script = host
    .compile(&Block::from(
        r#"table("order").where("status", "due").count()"#,
    ))
    .unwrap();
let due = script.eval(&Env::new()).unwrap();
```

The three pieces:

- **[`Vocab`](vantage_rhai::Vocab)**: anything that registers functions and types on an engine.
  `DataVocab` is the data vocabulary. `ServoVocab` (diorama) and `SceneryVocab` (vantage-ui) are
  others. A host can carry several. When two register the same name, the one registered last wins,
  so register a backend's extensions first and `DataVocab` after them (SurrealDB has its own `table`
  function, an alias for `ident`, which `DataVocab`'s `table(name)` must override).
- **[`Limits`](vantage_rhai::Limits)**: `Limits::Ui` caps a script at 500,000 operations, for
  anything the UI thread waits on. `Limits::background()` allows 50 million, for scripts on worker
  threads: agent tools, imports, faker sims. A script that runs past its limit fails instead of
  hanging the caller.
- **[`Env`](vantage_rhai::Env)**: the variables one evaluation sees. Slot evaluators push `row` and
  `self` this way. A host builds its `Env` per call, so the compiled script is shared and the
  variables are not.

## `DataVocab`

```rust,ignore
pub struct DataVocab {
    pub resolver: Option<TargetResolver>,
    pub terminals: Terminals,
}
```

`resolver` backs `table(name)`. Without one, `table` isn't registered at all, and scripts can only
work with handles the host gives them (`self`, `row`).

`terminals` picks which verbs exist on top of the narrowing verbs:

| `Terminals` | Constructor | Verbs | Used by |
|---|---|---|---|
| `Describe` | `DataVocab::describe(resolver)` | none: the script returns a handle and the host reads its description | `modify:`, reference build scripts, augmentation sources, query preview |
| `Read { limit }` | `DataVocab::read(resolver, limit)` | reads, and `record(id)` drafts whose `save()`/`delete()` throw | form `options:`, action predicates |
| `ReadWrite { limit, writes }` | `DataVocab::read_write(resolver)`, `DataVocab::read_write_with(resolver, limit, writes)` | reads, writes, and records | agent scripts, action bodies, form and wizard scripts, faker sims |

`limit` caps every `list()` and `ids()`. A script's own `limit(n)` still applies below it. `None`
means no cap. `count()` isn't capped by the host's limit: it counts the whole set, or at most `n`
when the script narrowed with `limit(n)`. `read_write(resolver)` is uncapped with writes allowed.

`writes` is `Writes::Allowed` or `Writes::Denied(message)`. A denied host still registers every
write verb, but each one throws `message`. That keeps the error readable: a script that calls
`delete` on a host with writes turned off gets the host's explanation ("writes are off for
agents") instead of Rhai's "function not found". A `Describe` host, by contrast, has no `count()`
or `list()`, and calling one is a "function not found" error.

## The resolver

`TargetResolver` is `Arc<dyn Fn(&str) -> Result<Vista> + Send + Sync>`. It is called each time a
handle resolves, and must return a **fresh, unconditioned** Vista: the handle applies its own steps
on top. This is the resolver the guide's tests use, over an in-memory store:

<!-- tested: rhai_guide::support::resolver (used by every rhai_guide test) -->
```rust,ignore
pub fn resolver(store: &MemoryStore) -> TargetResolver {
    let catalog = catalog(store);
    Arc::new(move |name: &str| {
        let metadata = catalog
            .get(name)
            .unwrap_or_else(|| VistaMetadata::new().with_id_column("id"));
        let table = catalog.store().table(name);
        let shell = MemoryTableShell::new(table, metadata, catalog.clone());
        Ok(Vista::new(name, Box::new(shell)))
    })
}
```

The resolver is where an application puts its policy, because `vantage-vista` only calls it. Some
examples from the codebase:

- A YAML catalog resolves through its spec resolver, so `table("order")` in a reference build
  script is the same Vista the spec declares.
- Faker sims resolve over the sim's memory store, and wrap each Vista so writes are counted for
  the engine's stats.
- Vantage UI's page resolver accepts a full catalog key or its leaf name, reads from the data
  source rather than a cache, and wraps every Vista in a write guard. The guard refuses writes on
  read-only tables, coerces reference columns, gives an id-less insert a generated id, sends writes
  through the table's Dio (so they reach open pages), and wraps every `ref(...)` target in the
  target table's own guard.

None of that is visible to the script. It calls `insert`, and the Vista it resolved decides what an
insert means.

## Async reads from a synchronous script

Rhai is synchronous. Vista reads and writes are `async`. Every terminal verb runs its future through
one function, [`vantage_vista::rhai::block_on`](vantage_vista::rhai::block_on):

- With a tokio runtime available (`Handle::try_current()` succeeds), it blocks on the runtime.
- Without one, it polls the future once on the current thread. Memory Vistas finish on the first
  poll, so faker sims and tests run with no runtime at all.
- A future that is still pending with no runtime (any network backend) is an error, "this data
  source needs a tokio runtime; run the script under spawn_blocking". It never hangs.

Blocking on a runtime from inside an async task panics in tokio, so a host that has a runtime
evaluates scripts on a blocking thread:

<!-- tested: rhai_guide::hosts::under_spawn_blocking -->
```rust,ignore
let rows = tokio::task::spawn_blocking(move || {
    let host = Host::builder(Limits::background())
        .vocab(DataVocab::read(Some(resolver), Some(2)))
        .build();
    host.compile(&Block::from(r#"table("order").list()"#))
        .and_then(|s| s.eval(&Env::new()))
        .map(|v| vantage_rhai::to_json(&v))
})
.await
.unwrap()
.unwrap();
```

[`run_script`](vantage_vista::run_script) does exactly this for you. Action bodies in Vantage UI
run under `Limits::Ui` on a background executor thread that has a runtime handle, which the bridge
also accepts.

## Errors

Script errors come back as Rhai errors whose text names what failed:

- a narrowing step the backend can't apply (an unknown relation, a column that isn't orderable, a
  computed column) fails when the handle resolves, at the terminal verb, not where the step was
  written. The error names the step: `Step can't be applied (step: Ref("invoices")): …`;
- a verb the Vista's capabilities don't cover names the verb and the table:
  "`delete` isn't supported by table `x`";
- backend errors keep their message.

Hosts that return JSON, like `run_script`, flatten all of these into the error string.
