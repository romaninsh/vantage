# Data Source

Because Vantage works with any arbitrary database - Data Source are defined as anything
that can persist Entity data in a serialised way. Vantage is built on a foundation, that
different DataSource vendors will have different capabilities.

## Hierarchy

In Vantage, there are different kinds of DataSource implementations.

1. DataSource - foundational trait, that does not implement any methods
2. QuerySource - capability of executing operations defined by Expressions
3. SelectSource - capability of handling Select queries
4. TableSource - usable with generic Table implementation, must also implement
   QuerySource and SelectSource

Lets start with a most basic one

## IndexMap (DataSource)

In rust IndexMap implements ordered hash map. Similar to array in PHP or object in JS.
A regular IndexMap can act as a data source and a pretty fast one! You may also call
this in-memory `kv` store.

DataSource which implements IndexMap is called `ImDataSource` and is defined in
`vantage-dataset` crate.

IndexMap has only 4 possible operations:

- add new record with a given ID.
- retrieve record (if present) for a given ID.
- edit and delete record with a given ID.
- get all records (in an undefined order).

In other words - DataSet associated with ImDataSource will be:

- InsertableDataSet (ds.insert())
- LoadableDataSet (ds.load())
- ReadableDataSet (ds.get())
- EditableDataSet (ds.patch(), ds.replace(), ds.delete())

## CSV (DataSource)

Vantage has a built-in CsvDataSet, which can read a CSV file for you. First row
contains keys and all other rows contain values. To initialize CsvDataSet you
should pass it a folder where multiple CSV files may live.

Default implementation will emit ReadableDataSet only:

```rust
ds = CsvDataSet::new("data/csv_folder");
let countries = ds.from::<Country>("countries.csv");
```

While you can only read from CSV, you can copy it into KV, like this:

```rust
csv = CsvDataSet::new("data/csv_folder");
cache = ImDataSource::new();

let countries = csv.from::<Country>("countries.csv");
let cached_countries = cache.from::<Country>("countries").import(countries);
```

While this is possible - this will assign random IDs to all countries during import.
Here is a better way to import and use a key:

```rust
let countries = csv.from::<Country>("countries.csv");
let cached_countries = cache.from::<Country>("countries")
    .import_map(countries, |c|(c.name, c));
```

While we use Country in both DataSet - import_map would also allow us to remap type too.

## RouterDataSet

Router Data Set allow you to redirect certain operations. RouterDataSet implements
Readable, Writable and InsertableDataSet traits, however you must link it up with an
underlying dataset:

```rust
let router = RouterDataSet(Some(cached_countries), None, Some(country_queue));
```
