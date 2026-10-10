//! Row and count reads for a `MongoSelect`: a `find` over the collection, or
//! an `aggregate` over a view or when the select carries leading stages.

use bson::{Bson, Document};
use futures_util::TryStreamExt;
use vantage_core::{Result, error};

use crate::mongodb::MongoDB;
use crate::select::MongoSelect;

impl MongoDB {
    /// Whether `select` reads through `aggregate`: over a view (even one
    /// with no stages), or when it carries leading stages or joins.
    pub(crate) fn reads_pipeline(&self, select: &MongoSelect) -> bool {
        self.view().is_some() || !select.pre_stages.is_empty() || !select.joins.is_empty()
    }

    fn select_collection(&self, select: &MongoSelect) -> Result<mongodb::Collection<Document>> {
        let name = select
            .collection
            .as_deref()
            .ok_or_else(|| error!("MongoSelect has no collection set"))?;
        Ok(self.read_collection(name))
    }

    /// Run `pipeline` over the collection `select` reads.
    pub(crate) async fn aggregate_docs(
        &self,
        select: &MongoSelect,
        pipeline: Vec<Document>,
    ) -> Result<Vec<Document>> {
        self.select_collection(select)?
            .aggregate(pipeline)
            .await
            .map_err(|e| error!("MongoDB aggregate failed", details = e.to_string()))?
            .try_collect()
            .await
            .map_err(|e| error!("MongoDB aggregate cursor failed", details = e.to_string()))
    }

    /// The documents `select` reads, filtered, sorted and paged.
    pub(crate) async fn read_docs(&self, select: &MongoSelect) -> Result<Vec<Document>> {
        if self.reads_pipeline(select) {
            let pipeline = select.as_read_pipeline().await?;
            return self.aggregate_docs(select, pipeline).await;
        }
        let filter = select.build_filter().await?;
        self.select_collection(select)?
            .find(filter)
            .with_options(select.build_find_options())
            .await
            .map_err(|e| error!("MongoDB find failed", details = e.to_string()))?
            .try_collect()
            .await
            .map_err(|e| error!("MongoDB cursor failed", details = e.to_string()))
    }

    /// `func` (`$sum`, `$max`, `$min`) over `field` across the documents
    /// `select` matches; `empty` when none match.
    pub(crate) async fn accumulate(
        &self,
        select: &MongoSelect,
        func: &str,
        field: &str,
        empty: Bson,
    ) -> Result<Bson> {
        let pipeline = select.as_aggregate_pipeline(func, field).await?;
        let docs = self.aggregate_docs(select, pipeline).await?;
        Ok(docs
            .first()
            .and_then(|d| d.get("val"))
            .cloned()
            .unwrap_or(empty))
    }

    /// The number of documents `select` matches, ignoring its paging.
    pub(crate) async fn count_docs(&self, select: &MongoSelect) -> Result<i64> {
        if self.reads_pipeline(select) {
            let pipeline = select.as_count_pipeline().await?;
            let docs = self.aggregate_docs(select, pipeline).await?;
            return Ok(match docs.first().and_then(|d| d.get("count")) {
                Some(Bson::Int32(n)) => i64::from(*n),
                Some(Bson::Int64(n)) => *n,
                _ => 0,
            });
        }
        let filter = select.build_filter().await?;
        let count = self
            .select_collection(select)?
            .count_documents(filter)
            .await
            .map_err(|e| error!("MongoDB count_documents failed", details = e.to_string()))?;
        Ok(count as i64)
    }
}
