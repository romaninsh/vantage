# Vista: import columns from related models after creation

**Today:** a Vista's projection is fixed when it is created. Dotted
traversal columns (`batch.name`, `batch.golf_course.name`) must be
declared table-side before the wrap: YAML tables list them in the spec's
columns, typed tables call `Table::with_imported_column` before
`from_table`. A consumer that discovers late that it needs a related
column (a vantage-ui page whose `params.columns` names `batch.name`) has
no way to ask the Vista for it — vantage-ui silently drops the column
(`crates/app/src/screen/pages/columns.rs:18`).

**Want:** a capability + a post-creation mutator on Vista:

- `VistaCapabilities::can_import_columns` — the driver advertises that it
  can extend the projection with columns reached through the table's
  declared relations.
- `Vista::import_column(&mut self, path: &str) -> Result<()>` — callable
  AFTER the vista is created. The shell resolves `path` against the
  table's relations, adds the join (SQL) or traversal projection
  (SurrealDB idiom path), and registers the extra column in the vista's
  metadata (read-only: `calculated` flag, not orderable where ordering by
  the alias cannot work). Errors loudly when the first path segment is
  not a declared relation or the driver lacks the capability.

Implementation sketch:

- SurrealDB shell: delegate to `Table::add_imported_column` (already
  additive) on the shell's inner table, then append the metadata column —
  the projection alias mechanism exists end to end.
- SQL shells: add a LEFT JOIN through the relation's foreign key and
  project `related.column AS "path"`.
- Drivers without a join/traversal story keep the capability off; callers
  fall back to declaring the column table-side.

**First consumer:** vantage-ui page loading. When a page's
`params.columns` names a dotted column the table does not project, the
loader calls `import_column` on the freshly built Vista (YAML-spec and
model-bundle paths converge here). That deletes the last UI-serving lines
from bundle `build_vista` implementations (see
`vantage-ui/crates/backend/src/bundles/chtags.rs`) and makes page YAML
self-sufficient: name a related column, get a join. Companion vantage-ui
notes: `chtags/agent/todo/vantage-ui-page-dotted-columns.md`.
