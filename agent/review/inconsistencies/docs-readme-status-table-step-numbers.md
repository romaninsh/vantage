# README backend-status table step numbers don't match the Persistence Guide it cites

- **Severity:** low
- **Category:** inconsistencies
- **Location:** `README.md:709-716`

The "Current status" table says it measures progress "against the steps in the [Persistence Guide](docs4/src/new-persistence.md)" and lists 6 steps: 1 Type system, 2 Expressions, 3 Query builder, 4 Table, 5 Relationships, 6 Multi-backend. The actual guide has 9 steps with different numbering — Step 3 is Operators, Step 4 is Query Builder, Step 5 is Table & CRUD, Step 6 is Relationships, Step 7 is Multi-Backend, plus Step 8 Vista Integration and Step 9 Contained Relations which the table omits entirely. Anyone cross-referencing a row to the named guide chapter lands on the wrong step.

```
| Step | Feature                                                 | SurrealDB | ...
| 3    | Query builder (`Selectable`, `SelectableDataSource`)    | Full      | ...
| 4    | Table abstraction (`TableSource`, CRUD, aggregates)     | Full      | ...
| 6    | Multi-backend (`AnyTable`, CLI)                          | Full      | ...
```

**Recommendation:** Renumber the table to match the guide's 9 steps (adding Vista Integration and Contained Relations rows), and replace the `AnyTable` mention in row 6 with the real multi-backend mechanism (Vista/CLI).
