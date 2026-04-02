use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::future::Future;

/// Represents a [`dataset`] that may can add or modify records.
/// The <E> type parameter represents a record type.
///
/// # Example of [`Table`] which implements WritableDataSet
/// ```
/// Client::table().insert(Client { name: "John".to_string() }).await?;
///
/// let peter_orders = Client::table().with_id(1).ref_orders();
/// peter_orders.update(|orders| orders.qty += 1).await?;
/// ```
///
/// [`dataset`]: super
/// [`Table`]: crate::table::Table
pub trait WritableDataSet<E> {
    /// Insert a new record into the DataSet.
    ///
    /// ```
    /// Client::table().insert(Client { name: "John".to_string() }).await?;
    /// ```
    fn insert(&self, record: E) -> impl Future<Output = Result<Option<Value>>>;

    /// Update all records in the DataSet. When working with Table, it's important to set a condition
    /// if you only want to update some records.
    ///
    /// ```
    /// let peter_orders = Client::table().with_id(1).ref_orders();
    /// peter_orders.update(|orders| orders.qty += 1).await?;
    /// ```
    fn update<F>(&self, f: F) -> impl Future<Output = Result<()>>;

    fn update_with<F, E2>(&self, values: E2) -> impl Future<Output = Result<()>>
    where
        E2: Serialize + Clone;

    /// Delete all records in the DataSet. When working with Table, it's important to set a condition
    /// if you only want to delete some records.
    ///
    /// ```
    /// let peter = Client::table().with_id(1);
    /// peter.ref_orders().delete().await?;    // delete all orders of peter
    /// peter.delete().await?;                 // delete peter
    ///
    /// ```
    fn delete(&self) -> impl Future<Output = Result<()>>;
}
