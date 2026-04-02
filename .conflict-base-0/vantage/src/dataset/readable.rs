use std::future::Future;

use crate::sql::Query;
use anyhow::Result;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

/// Represents a [`dataset`] that may be used to fetch data.
/// The <E> type parameter represents a record type.
///
/// # Example of [`Table`] which implements ReadableDataSet
/// ```
/// for client in Client::table().get().await? {
///     dbg(&client.name);
/// }
/// ```
///
/// [`dataset`]: super
/// [`Table`]: crate::table::Table
pub trait ReadableDataSet<E> {
    /// Fetch all records from the dataset and return them as a [`Vec<E>`].
    fn get(&self) -> impl Future<Output = Result<Vec<E>>>;

    /// Fetch one record from the dataset and return it as a `Vec<Map<String, Value>>`
    /// (where Map is [`serde_json::Map`])
    ///
    /// ```
    /// for client in Client::table().get_all_untyped().await? {
    ///     dbg!(&client["name"]?);
    /// }
    /// ```
    fn get_all_untyped(&self) -> impl Future<Output = Result<Vec<Map<String, Value>>>>;

    /// Fetch a single row only. This is similar to [`get_some`], but returns [`json::Map`].
    fn get_row_untyped(&self) -> impl Future<Output = Result<Map<String, Value>>>;

    /// Fetch a column of single untyped value, return it as a `Vec<Value>`.
    fn get_col_untyped(&self) -> impl Future<Output = Result<Vec<Value>>>;

    /// Fetch a single row and only one column. This makes sense if your DataSet is produced
    /// from fetching arbitrary set of columns, containing a single column only.
    ///
    /// ```
    /// let client = Client::table();
    /// let name_query = client.get_select_query_for_fields(["name"]);
    ///
    /// let name: Value = client.get_row_untyped(name_query).await?;
    /// ```
    fn get_one_untyped(&self) -> impl Future<Output = Result<Value>>;

    /// Fetch some record if dataset has at least one record. Works well with Table::with_id().
    ///
    /// ```
    /// let client = Client::table().with_id(1).get_some().await? else {
    ///     return Err(anyhow::anyhow!("No client with id=1"));
    /// };
    /// dbg!(&client.name);
    /// ```
    fn get_some(&self) -> impl Future<Output = Result<Option<E>>>;

    /// Fetch records into a vector of type `T` using [`serde_json::from_value`].
    ///
    /// ```
    /// struct ClientNameOnly {
    ///     name: String,
    /// }
    ///
    /// for client in Client::table().get_as::<ClientNameOnly>().await? {
    ///     dbg!(&client.name);
    /// }
    /// ```
    ///
    /// [`serde_json::from_value`]: serde_json::from_value
    fn get_as<T: DeserializeOwned>(&self) -> impl Future<Output = Result<Vec<T>>>;

    /// Fetch a single record into a type `T` using [`serde_json::from_value`].
    fn get_some_as<T>(&self) -> impl Future<Output = Result<Option<T>>>
    where
        T: DeserializeOwned + Default + Serialize;

    /// TODO: must go away from here, as dataset should not be aware of query
    fn select_query(&self) -> Query;
}
