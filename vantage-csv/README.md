# vantage-csv

CSV backend for the [Vantage](https://github.com/romaninsh/vantage) data framework.

Implements `TableSource` so that `Table<Csv, E>` works alongside
`Table<SurrealDB, E>` (or any other backend) for read operations.

## Type system

Like SurrealDB's `AnySurrealType` (backed by CBOR), vantage-csv defines
`AnyCsvType` (backed by `String`) via the `vantage_type_system!` macro.

Column definitions on the table drive type-safe parsing:

```rust
let table = Table::<Csv, EmptyEntity>::new("product", csv)
    .with_column_of::<String>("name")
    .with_column_of::<i64>("price")
    .with_column_of::<bool>("is_deleted")
    .with_column_of::<serde_json::Value>("inventory");
```

When reading a CSV row, each field is parsed according to its column type:
- `"300"` + column type `i64` → `AnyCsvType { value: "300", variant: Int }`
- `"true"` + column type `bool` → `AnyCsvType { value: "true", variant: Bool }`
- `"hello"` + no column defined → `AnyCsvType { value: "hello", variant: String }`

Values can be extracted type-safely:
```rust
let price: i64 = record["price"].try_get::<i64>().unwrap();
let name: String = record["name"].try_get::<String>().unwrap();
```

## Usage

```rust
use vantage_csv::Csv;
use vantage_table::table::Table;
use vantage_dataset::prelude::ReadableValueSet;

let csv = Csv::new("data/");
let products = Table::<Csv, MyProduct>::new("product", csv)
    .with_column_of::<String>("name")
    .with_column_of::<i64>("price")
    .into_entity::<MyProduct>();

let all = products.list().await?;
```

Each table reads from `{base_dir}/{table_name}.csv`. The first row is treated
as headers and every subsequent row becomes a record.

## ID column

By default the `id` column is used as the record identifier. Override with:

```rust
let csv = Csv::new("data/").with_id_column("code");
```

If the CSV has no matching column, row indices (starting at 0) are used instead.

## Embedded JSON

Object/array fields can be stored as quoted JSON strings in CSV:

```csv
id,name,inventory
1,Widget,"{""stock"":50}"
```

Define the column as `serde_json::Value` to parse it:
```rust
table.with_column_of::<serde_json::Value>("inventory")
```

## Read-only

CSV is a read-only data source. Write operations (`insert`, `replace`,
`patch`, `delete`) return errors at runtime.

## License

MIT OR Apache-2.0
