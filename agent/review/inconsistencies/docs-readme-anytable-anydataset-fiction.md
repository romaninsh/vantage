# README documents type-erasure API (AnyTable, AnyDataSet) that does not exist in code

- **Severity:** high
- **Category:** inconsistencies
- **Location:** `README.md:99` (also `README.md:213,224-231`)

The README's "0.4 key additions" claims `AnyTable::from_table()` wraps any datasource, the feature list claims "Type-erased structs for all major traits (AnyTable, AnyExpression)", and the "Type erasure support" section shows `AnyDataSet::new(clients)`. Neither `AnyTable` nor `AnyDataSet` exists anywhere in the workspace crates (`grep -r AnyTable vantage-*/src` only matches doc comments in vantage-aws). The actual erasure layer is `Vista` (`vantage-vista`, `T::vista_factory().from_table(...)` per `vantage-table/src/lib.rs:8`). Users following the README will look for types that were never published.

```
- **Cross-database integration** - `AnyTable::from_table()` wraps any datasource for generic code
...
let clients = Client::admin_api(); // impl DataSet<Client>
let clients = AnyDataSet::new(clients); // AnyDataSet - types erased.

let entities: = vec![clients, orders, ..];
```

**Recommendation:** Rewrite the type-erasure sections around `Vista`/`vantage-vista-factory` (the real API), or remove the `AnyTable`/`AnyDataSet` claims until such types exist. `AnyExpression` does exist and can stay.
