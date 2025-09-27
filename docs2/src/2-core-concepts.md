# Core Concepts

Let me introduce the "Bakery" example. This example will be used throughout this book
and it is a pretty simple to grasp.

The "Bakery" app works with number of physical bakeries throughout the country.
Each employes number of bakers and have tumber of clients. Bakery data must be
separate, even if they are stored in the same physical location.

This concept is co-locating data for various clients is called "multi-tenancy".
Bakery is a tenant, and while the data may share the same physical location -
separation between tenants is very important.

Some database systems which allow you to perform hard isolation, but
that's not always the case. Our bakeries actually share a common table called "products".

I'll tell more about the "Bakery" as we go through the concepts, but for now lets
introduce "The Entity".

## The Entity, The Table and The Column.

A Bakery SaaS startup could describe a "Product" as a table with 3 columns - `id`,
`name` and `description`. In larger organisation, even a simple table like that would
have over 20 columns, some old, some mis-used. Therefore in Vantage - we don't really
know what is the extend of entire record.

Instead, we operate with Entities. The entity is a Rust struct, which describes
a subset of columns for a given table. Of course - ideally - it would contain all the
fields, but in Vantage we simply say that - a same dataset can operate with different
Entities.

What that means in practice - someone else adding a new column would not affect the
software that is not aware of this column. Different database system have ways to
incororate optional columns. SurrealDB allows schemaless tables, SQL allow JSON fields,
etc.

What we do agree on - some fields would be obligatory and absolutely necessary when
adding a record and in Vantage we refer to them as "columns".

Now lets put some code together:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Product {
    pub name: String,
    pub description: String,
}

impl Entity for User {}

let product_table = Table::new("product", datasource)
    .with_column("name")
    .with_entity::Product();
```

## Few Important Notes

I've just introduced a syntax of how Vantage operates with Entities, Columns and Tables.
Vantage recognizes that actual records do not live on your laptop. They are stored
remotely and there are probably millions of them.

There are 3 things that pin down your entities to a physical location:

1. DataSource. This is your physical database along with the namespace.
2. Table name. Together with DataSource this uniquily pinpoints the data location.
3. ID. We recognise that every row has an ID. Also ID is always a String.

Permissions permitting - you can add, patch, delete, replace or list records
in a table.

You could say that some tables can exist without IDs, but Vantage sees those
as either not a stand-alone table or not a table but rather a DataSet.

Don't worry, there are perfect ways to deal with them in Vantage.

## The DataSet - a table without columns.

In Vantage in addition to a Table there is another construct called DataSet. Unlike
Table a DataSet cannot have any columns.

A DataSet is like a Vec<Entity>, however in Vantage records are stored remotely. Another
quality of a DataSet is that it's capabilities are defined by associated DataSource.

For example:

- CSV file is a DataSet with a DataSource CsvFile - you can read or add entities, but not edit.
- Queue publisher has a DataSource such as Kafka, but you can only add entities.
- The GET API Call is also a DataSet that has a RestAPI DataSource and you can only read entities.

To summarise - DataSource defines what can you do with a DataSet.

## Table and DataSet are interchangeable

In Vantage, many operations will look similar between DataSet or Table:

```rust
table.delete(id).await?;
dataset.delete(id).await?;
```

This allow you to switch a type of your record between DataSet and Table without rewriting
your code.

Additionally - both table and dataset will implement a Trait so you can implement a
generic method like this:

```rust
fn delete_some(impl EditableDataSet, id: String) {}
```
