# Changelog

## 0.4.5 — 2026-05-17

- Wires the stage-5 vocabulary to real [`Vista`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html) calls now that the methods have landed:
  - `?keyword` / `?"two words"` calls [`Vista::add_search`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_search).
  - `[+col]` / `[-col]` (and the sort half of `[+col:N]`) calls [`Vista::add_order`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_order).
- `field=value` is now coerced against the target column's declared type, not the value-shape heuristic. `name=true` on a `String` column produces `Text("true")`; `is_paying_client=true` on a `bool` column still produces `Bool(true)`. The `#literal` escape hatch still wins when the user wants to force a type. New [`vista_cli::coerce_for_column`](https://docs.rs/vantage-cli-util/0.4.5/vantage_cli_util/vista_cli/fn.coerce_for_column.html) exposes the same coercion for factory implementations that resolve locators to a typed id condition.
- Drivers without the matching `can_*` capability surface `Unsupported` from the underlying Vista call — same error contract as `Op::Eq`.
- Still stubbed (no Vista API yet): non-`eq` operator conditions (`:lt=`, `:gt=`, `:like=`, `:in=`, `:null`, `:notnull`), `[N:M]` range slicing (Vista's pagination is page-based, no offset-range primitive), and `@sum`/`@max`/`@min`/`@count` aggregates.
- Pins `vantage-vista = "0.4.9"`.

## 0.4.4 — 2026-05-17

- [`vista_cli`](https://docs.rs/vantage-cli-util/0.4.4/vantage_cli_util/vista_cli/index.html) grows the full stage-5 vocabulary as a parser. Operator conditions (`field:lt=`, `:gt=`, `:like=`, `:in=`, `:null`, `:notnull`), combined sort+slice brackets (`[+name:0]`, `[5:15]`, `[-salary]`), search (`?keyword`), aggregates (`@sum:price`, `@count`), and JSON-typed values (`field=#42`, `field=#"42"`, `field=#[1,2,3]`) all parse cleanly today. `Op::Eq` and `[N]` narrow-to-single drive real `Vista` calls; the rest dispatches to a `Renderer::note_stub` hook until `Vista` itself grows the corresponding methods, so callers can wire UIs against the final shape now.
- New [`output`](https://docs.rs/vantage-cli-util/0.4.4/vantage_cli_util/output/index.html) module: machine-readable formatters for `json`, `ndjson`, and `cbor-diag` (RFC 8949 §8 diagnostic notation). `cbor-diag` is the lossless format used for cross-driver golden tests; `json` is best-effort for `jq` pipelines.
- `ModelFactory::for_arn` is now `for_locator` — universal resource locators (`arn:…`, `user:abc123`, `urn:…`) instead of AWS-only. The old `for_arn` keeps working via a default forward, so existing implementations compile unchanged.
- `Renderer` gains `render_scalar` (for aggregate output) and `note_stub` (default no-op); existing implementors only need to add `render_scalar` when they exercise `@…` tokens.
- Module split: `vista_cli.rs` becomes `vista_cli/{mod,token,parse,value,factory,run}.rs`. All public items are re-exported, so downstream `use vantage_cli_util::vista_cli::{Mode, ModelFactory, Renderer}` keeps working.

## 0.4.3 — 2026-05-16

- `vista_cli::run` updated for [`Vista::get_ref`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/struct.Vista.html#method.get_ref)'s row-based signature: when a `:relation` token appears in `Mode::Single`, the runner now fetches the parent record via `get_some_value` and passes it to `get_ref(relation, &row)` instead of relying on conditions to project the join.
- After traversal the runner consults [`Vista::list_references`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/struct.Vista.html#method.list_references) and picks the render mode from the relation's declared cardinality — `HasOne` flips into `Mode::Single` (record card), `HasMany` stays `Mode::List` (grid). `:client` and `:bakery` render as cards; `:clients`, `:orders`, `:products` stay as tables.
- Pins `vantage-vista = "0.4.7"`.

## 0.4.2 — 2026-05-14

- New [`vista_cli`](https://docs.rs/vantage-cli-util/0.4.2/vantage_cli_util/vista_cli/index.html) module — the [`Vista`](https://docs.rs/vantage-vista/0.4.5/vantage_vista/struct.Vista.html) equivalent of `model_cli`. Same token grammar (`<model> [field=value ...] [[N]] [:relation ...] [=col1,col2]`), same `ModelFactory` / `Renderer` traits, but drives a `Vista` end-to-end so traversal goes through [`Vista::get_ref`](https://docs.rs/vantage-vista/0.4.5/vantage_vista/struct.Vista.html#method.get_ref) and condition pushdown reaches the driver's native condition type. Use this when you've migrated a CLI to the universal Vista surface; `model_cli` stays as the `AnyTable`-flavoured path.
- Pins `vantage-vista = "0.4.5"` for `Vista::get_ref` and `TableShell::get_ref`.

## 0.4.1 — 2026-04-30

- New `model_cli` module — generic, model-driven CLI runner over any `AnyTable`. Argv shape: `<model | arn> [field=value ...] [[N]] [:relation [[N]] ...] [=col1,col2 ...]`. Singular vs plural model names drive list/single mode; `id=value` is sugar that resolves to the table's id field and forces single-record mode; `[N]` selects the Nth record and switches into single-record mode by adding `eq(id_field, that_id)`; `:relation` traverses a registered `with_many` / `with_one`; `=col1,col2` overrides the visible columns in list mode. Glued forms (`users[0]`, `:members[0]`, `name=foo[0]`, `=msg,timestamp[0]`) split into a base token plus an index selector.
- Two new traits the caller implements: `ModelFactory` (resolve a name or ARN to an `AnyTable`) and `Renderer` (render lists / single records). The crate stays UI-/backend-agnostic — actual record rendering is delegated to the caller.
- New `render_records_columns` helper alongside the existing `render_records_typed`. Takes an explicit column list and does *not* auto-prepend an id column — for the case where the caller spelled out exactly what they want to see (e.g. the `=col1,col2` override path) and an extra id column would just be noise. Existing `render_records` / `render_records_typed` are unchanged.
- New `ciborium` dep (used at the model_cli boundary, where records flow as `Record<CborValue>`).
- Pins `vantage-table = "0.4.8"` for the new `TableLike` reflection methods (`id_field_name`, `title_field_names`, `column_types`, `get_ref_names`, `add_condition_eq`).

## 0.4.0 — 2026-04-16

- Initial 0.4 release. Generic CLI helpers (`render_records`, `handle_commands`) for driving any vantage backend through a uniform command set.
