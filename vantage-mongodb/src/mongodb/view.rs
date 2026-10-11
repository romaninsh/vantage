//! Read-only views: a `MongoDB` whose tables read the output of an
//! aggregation pipeline instead of a collection's stored documents, or whose
//! tables are server-side views.

use std::sync::Arc;

use bson::Document;
use vantage_core::{Result, error};

use super::MongoDB;

/// An aggregation pipeline over a collection. Tables over a `MongoDB`
/// carrying a view read the pipeline's output: their filter, sort and paging
/// run as stages after `stages`, and every write is refused. A row's id is
/// the `_id` the pipeline emits (for a `$group`, the group key), which must
/// be a string or an `ObjectId`; reshape any other key with a stage such as
/// `{ $addFields: { _id: { $toString: "$_id" } } }`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MongoView {
    /// Collection the pipeline runs against.
    pub collection: String,
    /// Leading pipeline stages, each a single-operator document.
    pub stages: Vec<Document>,
}

impl MongoView {
    /// A view running `stages` over `collection`.
    pub fn new(collection: impl Into<String>, stages: Vec<Document>) -> Self {
        Self {
            collection: collection.into(),
            stages,
        }
    }
}

impl MongoDB {
    /// This data source, with its tables reading `view` instead of their
    /// collection. Replaces any view already set.
    pub fn with_view(mut self, view: MongoView) -> Self {
        self.view = Some(Arc::new(view));
        self
    }

    /// This data source reading stored documents again.
    #[cfg(feature = "vista")]
    pub(crate) fn without_view(mut self) -> Self {
        self.view = None;
        self
    }

    /// The view tables over this data source read, if any.
    pub fn view(&self) -> Option<&MongoView> {
        self.view.as_deref()
    }

    /// The view's leading stages; empty without a view.
    pub(crate) fn view_stages(&self) -> Vec<Document> {
        self.view().map(|v| v.stages.clone()).unwrap_or_default()
    }

    /// The name of the collection reads of `table_name` run against: the
    /// view's collection when there is one.
    pub(crate) fn read_collection_name<'a>(&'a self, table_name: &'a str) -> &'a str {
        match self.view() {
            Some(view) => &view.collection,
            None => table_name,
        }
    }

    /// The collection reads of `table_name` run against.
    pub(crate) fn read_collection(&self, table_name: &str) -> mongodb::Collection<Document> {
        self.doc_collection(self.read_collection_name(table_name))
    }

    /// This data source, with its tables declared as server-side views
    /// (`db.createView`) or not. A server-side view serves reads, so writes
    /// and change streams are refused before reaching the server. Composes
    /// with [`with_view`](Self::with_view): a pipeline then runs over the
    /// server-side view. Tables built from a spec take the flag from
    /// `mongo.view` instead, so on a factory's `MongoDB` this affects only
    /// tables wrapped with `from_table`.
    pub fn with_server_view(mut self, server_view: bool) -> Self {
        self.server_view = server_view;
        self
    }

    /// Whether the tables over this data source are server-side views.
    pub fn is_server_view(&self) -> bool {
        self.server_view
    }

    /// Whether tables over this data source read and write stored documents:
    /// neither a pipeline view nor a server-side view.
    pub(crate) fn is_stored(&self) -> bool {
        self.view().is_none() && !self.server_view
    }

    /// Refuse a write to `table_name` when this data source carries a view.
    pub(crate) fn ensure_writable(&self, table_name: &str) -> Result<()> {
        if let Some(view) = self.view() {
            return Err(error!(
                "MongoDB pipeline view is read-only",
                collection = view.collection.clone()
            )
            .mark_unsupported());
        }
        if self.server_view {
            return Err(error!(
                "MongoDB server-side view is read-only",
                collection = table_name
            )
            .mark_unsupported());
        }
        Ok(())
    }

    /// Refuse a change stream on `table_name` when this data source carries
    /// a view: a view's rows are computed and have no change events of their
    /// own.
    #[cfg(feature = "vista")]
    pub(crate) fn ensure_watchable(&self, table_name: &str) -> Result<()> {
        if let Some(view) = self.view() {
            return Err(error!(
                "MongoDB pipeline view has no change stream",
                collection = view.collection.clone()
            )
            .mark_unsupported());
        }
        if self.server_view {
            return Err(error!(
                "MongoDB server-side view has no change stream",
                collection = table_name
            )
            .mark_unsupported());
        }
        Ok(())
    }
}

/// `CommandNotSupportedOnView`: the server refused a write or a change
/// stream because the namespace is a view.
const COMMAND_NOT_SUPPORTED_ON_VIEW: i32 = 166;

/// Whether the server refused an operation because the namespace is a
/// server-side view. A change stream fails with a command error; a write
/// reports the code as a write error, per document for `insert_many`.
fn is_view_error(e: &mongodb::error::Error) -> bool {
    use mongodb::error::{ErrorKind, WriteFailure};
    match &*e.kind {
        ErrorKind::Command(c) => c.code == COMMAND_NOT_SUPPORTED_ON_VIEW,
        ErrorKind::Write(WriteFailure::WriteError(we)) => we.code == COMMAND_NOT_SUPPORTED_ON_VIEW,
        ErrorKind::InsertMany(m) => m
            .write_errors
            .iter()
            .flatten()
            .any(|we| we.code == COMMAND_NOT_SUPPORTED_ON_VIEW),
        _ => false,
    }
}

/// The error for an operation on `collection` the server refused because
/// it is a view the table did not declare; `None` for any other error.
pub(crate) fn undeclared_view(
    e: &mongodb::error::Error,
    collection: &str,
) -> Option<vantage_core::VantageError> {
    is_view_error(e).then(|| {
        error!(
            "MongoDB collection is a view; declare `mongo: { view: true }` on the table",
            collection = collection
        )
        .mark_unsupported()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn offline() -> MongoDB {
        let client = mongodb::Client::with_uri_str("mongodb://localhost:27017")
            .await
            .unwrap();
        MongoDB::new(client, "unit")
    }

    #[tokio::test]
    async fn server_view_guards_refuse_before_the_server() {
        let db = offline().await.with_server_view(true);
        assert!(!db.is_stored());

        let err = db.ensure_writable("report").unwrap_err();
        assert!(err.is_unsupported(), "{err}");
        assert!(err.to_string().contains("server-side view is read-only"));
        assert!(err.to_string().contains("report"), "{err}");

        #[cfg(feature = "vista")]
        {
            let err = db.ensure_watchable("report").unwrap_err();
            assert!(err.is_unsupported(), "{err}");
            assert!(
                err.to_string()
                    .contains("server-side view has no change stream")
            );
        }

        let plain = db.with_server_view(false);
        assert!(plain.is_stored());
        assert!(plain.ensure_writable("report").is_ok());
    }
}
