# Changelog

## 0.1.2 — 2026-10-14

- The in-crate unit tests move to the blackbox suite under `tests/`; comments trimmed. No API or
  behaviour change.

## 0.1.1 — 2026-10-11

- `ensure_gitignore` returns an error for an unreadable `.gitignore` instead of overwriting it.
- `emit_schemas` removes only `<folder>-schema-<N>.json` files with an integer `N`.
- `emit_schemas` returns an error and deletes no old schemas when a YAML file can't be read.
- `reload_path` reloads the dependents of a deleted include given through a symlinked root.

## 0.1.0 — 2026-10-11

- YAML load pipeline: `load_document`, `parse_document`, `validate_into`, `load_yaml_validated`,
  `parse_yaml_validated`.
- `!include` of data files (`.yaml`, `.yml`, `.json`) and text files, confined to the project root;
  cycles and unknown tags are errors.
- `Document` carries the included paths and a hash over the file and its includes.
- Per-type schema cache (`schema_for_type`, `CachedSchema`).
- `Kind` trait and `Inventory` registry: folder and root-file kinds, one-level subfolders,
  cross-reference validation against a `KeySnapshot` and kind-contributed indexes, returning
  messages. `Kind::parse` gets the leaf's key.
- `Inventory::reload_path` returns a `Reload`: changes, failures and whether revalidation moved
  any errors. Include dependents, hash dedupe, serials that bump only on valid reloads, deletes,
  per-kind handling of broken edits (`OnFailure`).
- Failures are returned, not logged: `load` returns them too.
- `DynKind` for boxed kinds; `with_dyn`, `with_all`. Duplicate ids, folders or root files panic.
- `emit_schemas` (versioned schema files, READMEs, cleanup) returns an `EmitReport`;
  `ensure_gitignore` returns whether it wrote.
- `watch` feature: `watch(root, on_change)` calls back with each changed path or watcher error
  and returns a `Watcher` guard (pulls in `notify`).
- `jsonschema` without default features: no remote `$ref` resolution, no HTTP client.
