# Stage 8 — vantage-ui migration

Status: **Not started**

Migrate ../vantage-ui to consume `Vista` instead of the older type-erased table wrapper. Eliminates
the parallel column-threading workaround, the JSON↔CBOR adapter, the `is_api_backed` flag, and the
AWS-only condition asymmetry. vantage-ui's existing pain-point doc closes here.

## Discussion phase

- [ ] Migration strategy — parallel period (both old and new types coexist) vs hard cutover on a 0.5
      branch?
- [ ] Are there features in vantage-ui that depend on internals of the old type-erased wrapper that
      need to be re-exposed via Vista?
- [ ] Driver registration shape in vantage-ui — `Box<dyn VistaFactory>` per datasource, or
      per-driver concrete factory?
- [ ] Live-update wiring — does vantage-ui take Coop'd Vistas where it needs reactivity, plain
      Vistas otherwise? Or always Coop'd?
- [ ] Master/detail traversal — confirm portable conditions from stage 5 replace the AWS-only path
- [ ] Storybook (`widget-storybook`) implications — does it need mock factories for fixtures?

## Scope

In (vantage-ui-side changes):

- Replace 4× `build_*_table` dispatch in `app/src/backend/schema.rs` with a single
  `factory.from_yaml(yaml)?` call per config
- Drop `EntityBackend.columns: Vec<Arc<dyn ColumnLike>>` parallel field; read columns from the Vista
  directly
- Delete `app/src/backend/json_cbor_adapter.rs` (vantage-rest is now CBOR-native)
- Delete `components/src/schema_column.rs` shim
- Replace `is_api_backed: bool` with `vista.capabilities()` queries
- Replace AWS-only master/detail condition path with portable conditions
- Wire Rhai-defined hooks
- Wire reactive grid through Coop where applicable

Out:

- Decommissioning the old types in vantage workspace — that's stage 9
- New vantage-ui features — keep migration scope tight

## Plan

- [ ] Discuss with user: migration strategy, driver registration shape, Coop-vs-plain split,
      storybook impact
- [ ] Add `vantage-vista` and per-driver crate deps to vantage-ui
- [ ] Replace `build_sqlite_table` / `build_surreal_table` / `build_api_table` / `build_aws_table`
      with single dispatch over `Box<dyn VistaFactory>`
- [ ] Drop `JsonToCborAdapter`
- [ ] Drop `SchemaColumn` and `schema_columns()` mirror
- [ ] Drop `EntityBackend.columns` parallel field; grid reads from Vista
- [ ] Replace `is_api_backed` with capability queries; pick grid mode from
      `vista.capabilities().paginate_kind`
- [ ] Update master/detail traversal to use `vista.add_condition(...)` (universal path)
- [ ] Wire Coop'd Vistas where reactive UI is wanted; plain Vistas otherwise
- [ ] Update inventory YAML schema if needed; document migration of driver-specific extras blocks
- [ ] Update `app/todo/anytable-portable-conditions.md` — close it
- [ ] vantage-ui smoke test: load all bakery fixtures, render grids, master/detail traversal,
      search, pagination

## References

- Closes:
  - `/Users/rw/Work/vantage-ui/app/todo/anytable-portable-conditions.md`
- Subsumes (vantage-ui's perspective):
  - `EntityBackend.columns` parallel threading workaround
  - `JsonToCborAdapter` workaround
  - `SchemaColumn` shim
  - `is_api_backed` ad-hoc capability flag
  - 4× repeated YAML type mapping in `app/src/backend/schema.rs`
- Touches:
  - `../../FINAL_TODO.md` "Multi-tenancy patterns" — easier with universal hooks; sub-stage of
    follow-up work
