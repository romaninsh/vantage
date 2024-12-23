# v0.2 (eta January 2025)

- [x] Swap to sqlx
- [ ] Allow use of custom columns
- [ ] "returning `id` should properly choose ID column"
- [ ] Add thread safety (currently tests in bakery_api fail)
- [ ] Implement transaction support
- [ ] Add MySQL support
- [ ] Add a proper database integration test-suite
- [ ] Implement all basic SQL types
- [ ] Implement more operations
- [ ] Fully implement joins
- [ ] Implement and Document Disjoint Subtypes pattern
- [ ] Add and document more hooks
- [ ] Comprehensive documentation for mock data testing
- [ ] Implement "Realworld" example application in a separate repository
- [ ] Implement Uuid support
- [ ] with_id() shouldn't need into()

# v0.3

- [ ] Implement associated records (update and save back)
- [ ] Implement table aggregations (group by)
- [ ] Implement NoSQL support
- [ ] Implement RestAPI support
- [ ] Implement Queue support
- [ ] Add expression as a field value (e.g. when inserting)
- [ ] Add delayed method evaluation as a field value (e.g. when inserting)
- [ ] Add tests for cross-database queries
- [ ] Explore replayability for idempotent operations and workflow retries
- [ ] Provide example for scalable worker pattern

# Someday maybe:

- [ ] Implement todo in update() in WritableDataSet for Table
- [ ] Continue through the docs - align crates with documentation

# Create integration test-suite for SQL testing

- [ ] Create separate test-suite, connect DB etc
- [ ] Make use of Postgres snapshots in the tests
- [ ] Add integration tests for update() and delete() for Table

# Control field queries

- [ ] add tests for all CRUD operations (ID-less table)
- [ ] implemented `each` functionality for DataSet
- [ ] implement functions: (concat(field, " ", otherfield))
- [ ] move postgres integration tests into a separate test-suite
- [ ] add tests for table conditions (add_condition(field1.eq(field2))
- [ ] implement sub-library for datasource, supporting serde
- [ ] add second data-source (csv) as an example
- [ ] add sql table as a dataset at a query level (+ clean up method naming)
- [ ] postgres expressions should add type annotation into query ("$1::text")

Implement extensions:

- [ ] Lazy table joins (read-only)
- [ ] Implement add_field_lazy()

Minor Cases:

- [ ] Table::join_table should preserve conditions on other_table
- [ ] Table::join_table should resolve clashes in table aliases
- [ ] Condition::or() shouldn't be limited to only two arguments
- [ ] It should not be possible to change table alias, after ownership of Fields is given

## Implement cross-datasource operations

Developers who operate with the models do not have to be aware of the data source.
If you want to implement this, then you can define your data sets to rely on
factories for the data-set:

```rust
let client_set = ClientSet::factory();
let client = client_set.load_by_auth(auth_token)?;
let basket = client.basket();  // basket is stored in session/memory
for item in basket.items()?.into_iter() {
    let stock = item.stock();
    stock.amount -= item.amount;
    stock.save();  // saves into SQL

    item.status = "sold";
    item.save();   // item data is stored in cache
}
basket.archive();  // stores archived basked into BigQuery
```

## Implement in-memory cache concept

This allows to create in-memory cache of a dataset. Finding a record
in a cache is faster. Cache will automatically invalidate items if
they happen to change in the database, if the datasource allows
subscription for the changes. There can also be other invalidation
mechanics.

Cache provides a transparent layer, so that the business logic code
would not be affected.

```rust
let client_set = ClientSet::new(ClientCache::new(postgres));
// use client_set as usual
```
