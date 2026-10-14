# vantage-redb

Embedded [redb](https://crates.io/crates/redb) backend for the
[Vantage](https://github.com/romaninsh/vantage) data framework.

Implements `TableSource` so that `Table<Redb, E>` reads and writes a local redb file. Every write
runs in one redb write transaction, and secondary indexes are updated in the same transaction as
the row.

## Main types

- `Redb`: the data source. `Redb::create(path)` opens or creates a file, `Redb::open(path)` opens
  an existing one, `Redb::from_database(db)` wraps a `redb::Database` you built yourself. It is
  cheap to clone.
- `AnyRedbType`: the value type. Rows are stored as CBOR and keep their `RedbTypeVariants` tags
  through a round trip.
- `RedbCondition` and `RedbOperation`: conditions, built with `.eq(..)` and `.in_(..)` on a
  column.

## Usage

```rust
use vantage_dataset::prelude::*;
use vantage_redb::{AnyRedbType, Redb, RedbOperation};
use vantage_table::column::core::Column;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

let db = Redb::create("data.redb")?;
let users = Table::<Redb, EmptyEntity>::new("users", db)
    .with_id_column("id")
    .with_column_of::<String>("name")
    .with_column(Column::<String>::new("group").with_flag(ColumnFlag::Indexed));

let mut alice: Record<AnyRedbType> = Record::new();
alice.insert("name".into(), AnyRedbType::new("Alice".to_string()));
alice.insert("group".into(), AnyRedbType::new("admin".to_string()));
users.insert_value("a", &alice).await?;

let mut admins = users.clone();
admins.add_condition(admins["group"].clone().eq("admin"));
let rows = admins.list_values().await?;
```

Each indexed column gets its own redb table keyed by `(value, id)`, so an index can hold many rows
per value. `with_one` / `with_many` traversal resolves to an `in_` lookup on the target table, so
the target's foreign-key column must be indexed (or be its id column).

## Limits

- Conditions are `eq` / `in_`. The first condition must be on an indexed column or on the id
  column (which reads the row directly), otherwise the read panics with "non-indexed column".
  Later conditions are checked in memory and can name any column.
- No query builder and no ordering. Pagination is applied after the
  rows are loaded. Search and `sum` / `max` / `min` return errors. Counting works.
- No Vista support yet, so no live updates.

## License

MIT OR Apache-2.0
