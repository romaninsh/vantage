# README flips Table generic parameter order and shows outdated with_many closure shape

- **Severity:** medium
- **Category:** inconsistencies
- **Location:** `README.md:293-297` (also `README.md:137,146,540-547`)

The code defines `Table<T, E>` with the datasource first (`vantage-table/src/table/base.rs:18`), and the README's first example correctly writes `Table<SurrealDB, Client>` (line 137). But the "Table" and "Table References" sections then use the opposite order — `Table<Client, Oracle>`, `Table<Order, MongoDB>`, `Table<Order, SurrealDB>`, `Table<Client, SurrealDB>` — which won't type-check and confuses readers about which parameter is the entity. Additionally, line 540 shows `with_many("orders", "client_id", || Box::new(Order::table()))`, but the actual signature takes `impl Fn(T) -> Table<T, E2>` (no `Box`, closure receives the datasource: `.with_many("orders", "client_id", Order::postgres_table)` per the method's own doc example).

```
impl Client {
    fn table() -> Table<Client, Oracle>;
}
impl Order {
    fn table() -> Table<Order, MongoDB>;
}
...
let client = client.with_many("orders", "client_id", || Box::new(Order::table()));
```

**Recommendation:** Normalize all README examples to `Table<Source, Entity>` order and update `with_many`/`with_one` calls to the current `Fn(T) -> Table<T, E2>` closure form.
