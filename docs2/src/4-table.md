# Databases (or DataSource for Tables)

As I have mentioned before, DataSet has no columns. To work with columns
you need to use Table. Similarly to DataSets - tables must associate with
DataSource, however Table has higher requirements towards DataSet.

Table also implements all the features of a DataSet, therefore anywhere
where you can potentially use DataSet - you can also use Table.

When you design your code, however, if functionality of DataSet is sufficient,
you should use that:

```rust
fn build_ui_form(ds: impl InsertableDataSet, entity: impl Entity)
```

Ok, now lets talk Tables and generics first.

## Table and Rust type overloading

Rust is amazing. While there are languages out there, which allow you to
overload prototypes:

```
"somestring".custom_action();
```

Rust allow you to overload generic types too:

```
let a: Vec<&str> = vec!["a", "b"];
a.custom_action();
```

You can define `custom_action()` for Vec<&str> and not Vec<i64>. Vantage uses this
extensively when working with tables:

```
let countries = Table::new(ds, "countries");
let eu_countries = countries.only_eu();
```

Also quite commonly we will define table like this:

```
let countries = Country::table();
```

This is similar to Country::new() however easy to read that `countries` would be not a
single country but rather a table of countries. It's common to use locks/statics for
referencing database, i'll show some best-practice patterns later.

## Basic operations with table

Now since I mentioned already that table is compatible with DataSets, then the following
is possible:

```rust
let countries = Country::table();
countries.insert(Country { name: "Latvia".to_string() }).await?;
countries.insert_with_id("ru", Country { name: "Russia".to_string() }).await?;
let c = countries.load_by_id("ru").await?;
c.name="Ruzzia";
countries.save_with_id("ru", c).await?;
countries.delete("ru").await?;
countires.map(|c|{c.name = format("{} #peace", c.name); c}).await?;
let vec_c = countries.get().await?;
```

However Table has some cool things, like conditions:

```rust
let countries = Country::table()
  .with_column("is_eu");

let eu_countries = countries.clone().with_condition(countries["is_eu"].eq(true));
let vec_eu = eu_countries.get().await?;
```

However with a better table definition like this:

```rust
impl Country {
    pub fn table() -> Table<SurrealDB, Country> {
        Table::new("country", surrealdb())
            .with_column("is_eu")
            .into_entity()
    }
}
pub trait CountryTable {
    fn is_eu(&self) {
        self["is_eu"]
    }
    fn eu_only(&self) -> SurrealColumn<bool> {
        self.clone().with_condition(self.is_eu())
    }
}
impl CountryTable for Table<SurrealDB, Country> {}
```

Now you can write without fear of a field name typo and auto-complete:

```rust
let countries = Country::table();

let eu_countries = countries.clone().with_condition(countires.is_eu().eq(true));
// or
let eu_countries = countries.only_eu();
```

More importantly `["is_eu"]` returns a generic Box<ColumnLike> while eu_only() returns
a much more versatile column with type.
