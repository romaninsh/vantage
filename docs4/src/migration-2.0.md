# Migrating to 2.0

2.0 rebuilds the interface of every layer above `Table<T, E>`: `vantage-vista`, `vantage-diorama`
and every driver's `TableShell`. Two crates are gone, about twenty shell methods are gone, the Lens
has four settings instead of eight hooks, and a Dio no longer changes after it is built. This guide
maps each removed or renamed item to its replacement, page by page.

A typical port is mostly mechanical: fix the imports, rename the calls the compiler points at, then
read the behaviour notes for the areas your code leans on. The behaviour notes matter more than the
renames. A rename fails to compile; a changed default compiles and then does something else.

## What changed and why

1.x had grown several ways to do most things, and a few ways to do them silently wrong: a Dio
narrowed in place changed what every other holder saw, a paged view dropped the conditions its
loader didn't understand, a capability flag could claim a method the shell didn't implement. 2.0
sorts every public type into one of three kinds and gives each kind one way to change:

| Kind | Types | Narrowed by | Changed by |
|---|---|---|---|
| Value | `Table<T, E>`, `Vista`, `VistaQuery`, `Reference`, `ChangeFlash` | `clone().with_*()`, a new value | `add_*` / `set_*` on a value you own |
| Shared handle | `Lens`, `Dio` | `with_*` or `derive()`, a new handle | never after its builder |
| View state | `TableScenery`, `RecordScenery`, `ValueScenery`, `Servo` | a builder opens a new one | `set_*`, by its one owner |

Eleven rules hold that shape in place. The ones a migration runs into most:

- **One way to do each thing.** No `_counted`, `_ordered`, `_with` or `_sugared` variants; a
  variant is a builder option or a parameter.
- **No silent defaults.** A trait method either does the work or returns an `Unsupported` error.
- **Capabilities map one-to-one to methods**, and a contract test calls each advertised one.
- **Narrowing is never in place on shared objects.** `&self` on a Dio never changes what another
  holder sees.
- **`#[non_exhaustive]` on every public enum and data struct that may grow.** Matches gain a
  `_ =>` arm; structs are built through constructors.
- **One path per item.** A type is reachable from its module or the crate root, never both, and a
  crate re-exports nothing from another crate outside its prelude.

The full list is in the spec (`agents/specs/2026-10-11-vista-2.0/00-overview.md`). Each page below
opens with the rule or decision that drove its changes.

## Crates and features

| 1.x | 2.0 |
|---|---|
| `vantage-vista-factory` | deleted; `vantage_vista::Catalog` ([references, catalog and rhai](./migration-2.0/references-catalog-rhai.md)) |
| `vantage-diorama-aggregate` | deleted; `vantage_diorama::aggregate` ([aggregates](./migration-2.0/aggregates.md)) |
| `vantage_vista::mocks` always compiled | the `mocks` feature |
| `vantage_table::mocks`, `MockTableSource` always compiled | the `mocks` feature |
| `DebugTap`, `stats`, `Dio::diagnostics` always compiled | vantage-diorama's `diagnostics` feature |
| `Dio::take_write_worker_handle` (`#[doc(hidden)]`) | vantage-diorama's `test-util` feature |

Neither deleted crate has a shim. Drop both from `Cargo.toml`, and turn on the features the code
reads:

```toml
[dependencies.vantage-diorama]
features = ["diagnostics"]   # the debug stream, stats, Dio::diagnostics

[dev-dependencies.vantage-vista]
features = ["mocks"]         # MockShell
```

## How to read this guide

| Page | Covers |
|---|---|
| [Vista](./migration-2.0/vista.md) | reads, conditions, order and search, capabilities, metadata, the `TableShell` trait |
| [References, catalog and rhai](./migration-2.0/references-catalog-rhai.md) | `Reference` constructors, `Catalog`, resolvers, the rhai host API and script verbs |
| [Dio and Lens](./migration-2.0/dio-lens.md) | the Lens builder, loaders and refreshers, building and deriving a Dio, cache identity |
| [Writes and ingress](./migration-2.0/writes-ingress.md) | `Dio::flash`, the writer, awaited facade writes, confinement, `apply_change`, `reconcile` |
| [Sceneries](./migration-2.0/sceneries.md) | table, record and value sceneries: builders, setters, owned views, statuses |
| [Aggregates](./migration-2.0/aggregates.md) | `vantage_diorama::aggregate`, value sceneries, derived Dios, `Where` matching |
| [Surface](./migration-2.0/surface.md) | preludes, private modules, `#[non_exhaustive]`, features |
| [Drivers](./migration-2.0/drivers.md) | spec resolvers, `keys:` and column `references:`, quicksearch, per-driver notes |
| [Downstream](./migration-2.0/downstream.md) | what the vantage-ui and vantage-actions port has to change |

Each change shows a "before" and an "after". Every "before" block is compiled by the book's tests
and must fail with the error code on its fence, so a "before" that started compiling again would
fail the build. A rename with no behaviour behind it goes in a table instead. If you are porting an
application, start with [Surface](./migration-2.0/surface.md): most first-round compile errors are
import paths, and the rest of the guide reads more easily once they are gone.

### Why not keep the old name as an alias?

Because an alias is a second way to do the thing, forever. Rule 1 exists because 1.x had collected
exactly those: `fetch_page` beside `fetch_window`, `flash_patch` beside `flash`, `make_dio` beside
`make_dio_as`. Each pair looked harmless on its own, and together they meant two code paths to keep
correct, two spellings in every example, and a reader who couldn't tell which was preferred.

The usual answer is a deprecation cycle: keep the old name for a release, warn, then remove it.
That is the right tool inside a major version, and a deprecated `clone_shell` would have been the
right call for a 1.x release. 2.0 is a breaking release on purpose, and its one chance to remove
the second ways without another major version. The
compiler error is the migration checklist: each removed name fails at a line, and this guide
names the replacement. A shim would turn that error into a warning that gets ignored, and leave
the old path in the API for as long as anyone still calls it.

## Checklist

- [ ] `vantage-vista-factory` and `vantage-diorama-aggregate` are out of every `Cargo.toml`
- [ ] the crates that use `MockShell`, `DebugTap`, stats or `Dio::diagnostics` enable `mocks` /
      `diagnostics`
- [ ] the code compiles against the [Surface](./migration-2.0/surface.md) rules: no private module
      paths, no prelude imports of items that left it
- [ ] each page's checklist is done for the areas the code uses
- [ ] the behaviour notes on [Writes and ingress](./migration-2.0/writes-ingress.md) and
      [Sceneries](./migration-2.0/sceneries.md) are checked against the code that waits for a
      write or reads a view's state
