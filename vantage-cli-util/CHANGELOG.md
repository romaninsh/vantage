# Changelog

## 0.4.1 — 2026-04-30

- New `model_cli` module — generic, model-driven CLI runner over any `AnyTable`. Argv shape: `<model | arn> [field=value ...] [[N]] [:relation [[N]] ...] [=col1,col2 ...]`. Singular vs plural model names drive list/single mode; `id=value` is sugar that resolves to the table's id field and forces single-record mode; `[N]` selects the Nth record and switches into single-record mode by adding `eq(id_field, that_id)`; `:relation` traverses a registered `with_many` / `with_one`; `=col1,col2` overrides the visible columns in list mode. Glued forms (`users[0]`, `:members[0]`, `name=foo[0]`, `=msg,timestamp[0]`) split into a base token plus an index selector.
- Two new traits the caller implements: `ModelFactory` (resolve a name or ARN to an `AnyTable`) and `Renderer` (render lists / single records). The crate stays UI-/backend-agnostic — actual record rendering is delegated to the caller.
- New `render_records_columns` helper alongside the existing `render_records_typed`. Takes an explicit column list and does *not* auto-prepend an id column — for the case where the caller spelled out exactly what they want to see (e.g. the `=col1,col2` override path) and an extra id column would just be noise. Existing `render_records` / `render_records_typed` are unchanged.
- New `ciborium` dep (used at the model_cli boundary, where records flow as `Record<CborValue>`).
- Pins `vantage-table = "0.4.8"` for the new `TableLike` reflection methods (`id_field_name`, `title_field_names`, `column_types`, `get_ref_names`, `add_condition_eq`).

## 0.4.0 — 2026-04-16

- Initial 0.4 release. Generic CLI helpers (`render_records`, `handle_commands`) for driving any vantage backend through a uniform command set.
