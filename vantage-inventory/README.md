# vantage-inventory

Loads YAML inventory files into typed Rust structs. Every file goes through the
same pipeline: read, parse, expand `!include`, hash, convert to JSON, validate
against the target type's JSON schema, deserialize.

```toml
[dependencies]
vantage-inventory = "0.1.1"
```

```rust
use vantage_inventory::load_yaml_validated;

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Page {
    title: String,
    script: Option<String>,
}

let page: Page = load_yaml_validated(&root.join("pages/home.yaml"), &root)?;
```

## `!include`

A value tagged `!include <path>` is replaced by the file it names. The path is
relative to the including file and must stay inside the project root.
`.yaml`, `.yml` and `.json` files are spliced in as data, with their own
includes expanded. Any other file becomes one string, which suits scripts,
queries and descriptions kept beside the YAML. Any other tag is an error.

`load_document` returns a `Document` with the tag-free value, the list of
included files and a hash over all of them, so a watcher can reload a document
when one of its includes changes.

## Schemas

`schema_for_type::<T>()` builds and caches the schema once per type.

## Projects: kinds and the `Inventory`

A project is a folder of YAML files grouped by kind: `table/*.yaml`,
`datasource/*.yaml`, a root `application.yaml`, and so on. Each kind implements
`Kind`, which says where its files live (`Location::Folder` or
`Location::RootFile`), how a `Document` parses into its value, and which
cross-references it checks.

```rust
use vantage_inventory::{Depth, Document, Inventory, KeySnapshot, Kind, Location};

struct TableKind;

impl Kind for TableKind {
    type Value = Table;
    fn id(&self) -> &'static str { "table" }
    fn location(&self) -> Location {
        Location::Folder { name: "table", depth: Depth::OneLevel }
    }
    fn parse(&self, _key: &str, doc: &Document) -> anyhow::Result<Table> {
        vantage_inventory::validate_into(doc)
    }
    fn schema(&self) -> anyhow::Result<serde_json::Value> {
        Ok(vantage_inventory::schema_for_type::<Table>()?.json.clone())
    }
    fn validate(&self, key: &str, t: &Table, keys: &KeySnapshot) -> Vec<String> {
        if keys.has("datasource", &t.datasource) { return vec![]; }
        vec![format!("{key}: unknown datasource `{}`", t.datasource)]
    }
}

let mut inv = Inventory::new(&root).with(TableKind).with(DatasourceKind);
let failures = inv.load()?;
let table = inv.get::<TableKind>("bakery-surreal/bakery");
```

- Keys are file stems; a `Depth::OneLevel` folder also reads one level of
  subfolders, keyed `<sub>/<stem>`.
- A file that does not load or parse adds no leaf. The inventory logs nothing:
  `load` returns the `Failure`s for the caller to report. Validation problems
  do not block a leaf: they show in `errors_for`.
- Registering two kinds with the same id, folder or root file panics.
- `reload_path` updates the store for one changed file and returns a
  `Reload`: the `Change`s, the `Failure`s, and whether revalidation changed
  any leaf's errors. Leaves that `!include` the file reload too. Saving
  identical content changes nothing, and a leaf's serial only bumps when the
  new version parses and validates.
- Validators read a `KeySnapshot`; a test can build one with
  `KeySnapshot::new` and an `IndexSink`.
- `emit_schemas(version)` writes `<folder>/<folder>-schema-<version>.json` and
  the kind's README, deletes older schema versions no file references, and
  returns an `EmitReport` of the files it wrote and removed.
  `ensure_gitignore` keeps a managed block of the caller's lines in
  `.gitignore` and returns whether it wrote.
- Kinds of different value types fit in one `Vec<Box<dyn DynKind>>`; register
  them with `with_all`.

## Features

- `watch`: `watch(root, on_change)` starts a recursive file watcher and
  returns a `Watcher` guard (dropping it stops the watch). `on_change` runs on
  the watcher's thread with each changed path to feed to `reload_path`, or
  with the error the backend reported.
