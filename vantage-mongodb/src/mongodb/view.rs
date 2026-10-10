//! Read-only views: a `MongoDB` whose tables read the output of an
//! aggregation pipeline instead of a collection's stored documents.

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

    /// Refuse a write when this data source carries a view.
    pub(crate) fn ensure_writable(&self) -> Result<()> {
        match self.view() {
            Some(view) => Err(error!(
                "MongoDB pipeline view is read-only",
                collection = view.collection.clone()
            )
            .mark_unsupported()),
            None => Ok(()),
        }
    }

    /// Refuse a change stream when this data source carries a view: the
    /// pipeline's rows are computed and have no change events of their own.
    #[cfg(feature = "vista")]
    pub(crate) fn ensure_watchable(&self) -> Result<()> {
        match self.view() {
            Some(view) => Err(error!(
                "MongoDB pipeline view has no change stream",
                collection = view.collection.clone()
            )
            .mark_unsupported()),
            None => Ok(()),
        }
    }
}
