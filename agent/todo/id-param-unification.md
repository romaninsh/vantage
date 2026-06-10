# Deferred: unify id-parameter convention across dataset/valueset traits

- **Origin finding:** `core-id-param-conventions-and-doc-drift` (severity: low)
- **Status:** deferred — the entity-level half was attempted and reverted because
  the value-level half has a workspace-wide blast radius. The doc-drift half is
  already fixed (see below).

## What's already done (do NOT redo)
- `ValueSet` doc example in `vantage-dataset/src/traits/valueset.rs` was corrected
  to the real signatures (`get_value -> Result<Option<Record<..>>>`,
  `get_some_value -> Result<Option<(Id, Record<..>)>>`). The doc-drift part of the
  finding is resolved.
- `TableLike` was dropped entirely (it was dead — only `dyn`-used in a disabled
  test + an undeclared example, plus a bound in `vantage-surrealdb`). Its removal
  unblocks making the `ValueSet` methods generic (see "Why deferred").

## The change to make
Standardize every id parameter on `impl Into<Self::Id> + Send`, matching the
existing `ReadableDataSet::get`. Specifically:

- `vantage-dataset/src/traits/dataset.rs`: `WritableDataSet::insert/replace/patch`
  (`&Self::Id` → `impl Into<Self::Id> + Send`).
- `vantage-dataset/src/traits/valueset.rs`: `ReadableValueSet::get_value` and
  `WritableValueSet::insert_value/replace_value/patch_value/delete`
  (`&Self::Id` → `impl Into<Self::Id> + Send`). (`get_value_record` in the
  `ActiveRecordSet` auto-impl then calls `get_value(id.clone())`.)
- Each implementor mirrors the signature and rebinds `let id = id.into();` at the
  top, passing `&id` to whatever lower layer it delegates to. Implementors:
  - `vantage-dataset/src/im/{dataset_writable,valueset_writable,valueset_readable}.rs`,
    `vantage-dataset/src/mocks/csv.rs` (note: `Id = usize`, so `idx == id`).
  - `vantage-table/src/table/sets/{writable_dataset,writable_value_set,readable_value_set}.rs`.
  - `vantage-vista/src/impls/{writable_value_set,readable_value_set}.rs`.
- Internal ref-site fixes: `vantage-dataset/src/record.rs`
  (`ActiveEntity::{save,delete}`, `ActiveRecord::save` → `self.id.clone()`),
  `vantage-table/src/table/impls/refereces.rs:~183`
  (`patch_value(parent_id, ...)`).

## Why deferred (the real cost)
`impl Into<Self::Id>` makes the methods generic. That is fine for the traits
themselves, but **every call site passing `&id` stops compiling** — `&String`
does not implement `Into<String>`. Sweeping the workspace, this is **~200 call
sites across ~15 crates**, including non-test source in `vantage-diorama`
(`src/dio/worker.rs`, `src/dio/mod.rs`, `src/scenery/*`, `src/lens/*`),
`vantage-vista/src/contained.rs`, plus large test suites in `vantage-sql`,
`vantage-mongodb`, `vantage-redb`, `vantage-csv`, `vantage-aws`, `vantage-cmd`,
`learn-3`, `bakery_model3/4`, `example_for_website`.

Most are mechanical (`&"x".to_string()` → `"x".to_string()`), but a meaningful
minority are `&var` where `var` is reused, requiring `.clone()` — not safely
regex-able. For a severity-low consistency nit this is disproportionate to do in
one pass, hence deferred.

## Recommended execution when picked up
1. Land the trait + implementor signature changes (the lists above).
2. Sweep call sites crate-by-crate, building each, letting the compiler enumerate
   breakage. For the bulk literal pattern, a guarded codemod is fine; hand-fix the
   `&var`-reused cases with `.clone()`.
3. `vantage-surrealdb` still needs its own migration to the current `TableSource`
   API (it's pre-migration); fold the id-param sweep into that.
4. Consider whether `ActiveRecordSet::get_value_record(&Self::Id)` and
   `ActiveEntitySet::get_entity(&Self::Id)` should also move to `impl Into` for
   full uniformity (left `&Self::Id` here).
