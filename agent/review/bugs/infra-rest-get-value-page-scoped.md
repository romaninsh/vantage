# RestApi get_table_value/get_table_count only see the current page

- **Severity:** medium
- **Category:** bugs
- **Location:** `vantage-api-client/src/rest/table_source.rs:131`

`get_table_value` fetches the table's list (with the table's pagination applied) and then looks the id up in the returned map. If the requested record is on any other page — or beyond the API's default page size — the lookup returns `Ok(None)` even though the record exists, with no error to distinguish "not found" from "not on this page". `get_table_count` (line 170) likewise returns the size of the fetched page, not the dataset's total. PoolApi at least fetches all pages, but pays a full-table scan per `get_table_value` call.

```rust
let records = self
    .fetch_records(
        table.table_name(),
        id_field_name(table).as_deref(),
        table.pagination(),
        table.conditions(),
    )
    .await?;
Ok(records.get(id).cloned())
```

**Recommendation:** Issue an id-targeted request instead (`GET {base}/{table}/{id}` or `?{id_field}={id}` plus the existing conditions), and for count use the API's total field when the response shape carries one, otherwise document the page-scoped semantics.
