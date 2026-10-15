//! Aggregation pipeline builders for MongoSelect. Every pipeline starts with
//! `pre_stages`, then the filter as `$match`. Join stages run as late as the
//! read allows: after the conditions on stored fields, and after paging too
//! unless a condition or the sort reads a joined column.

use bson::{Bson, Document, doc};

use super::MongoSelect;
use super::join::filter_uses_joins;
use crate::condition::merge_documents;

impl MongoSelect {
    /// The filter split into its conditions on stored fields and those
    /// reading a joined column.
    async fn split_filter(&self) -> vantage_core::Result<(Document, Document)> {
        let (mut stored, mut joined) = (Vec::new(), Vec::new());
        for condition in &self.conditions {
            let resolved = condition.resolve().await?;
            if filter_uses_joins(&resolved, self.joins.len()) {
                joined.push(resolved);
            } else {
                stored.push(resolved);
            }
        }
        Ok((merge_documents(stored)?, merge_documents(joined)?))
    }

    /// The filter as `$match` stages: one on stored fields, one on joined
    /// columns; `None` where there are no such conditions.
    async fn match_stages(&self) -> vantage_core::Result<(Option<Document>, Option<Document>)> {
        let (stored, joined) = self.split_filter().await?;
        let stage = |filter: Document| (!filter.is_empty()).then(|| doc! { "$match": filter });
        Ok((stage(stored), stage(joined)))
    }

    /// `pre_stages` and the stored-field `$match`; then, when a condition
    /// reads a joined column, the join stages and the joined `$match`.
    async fn filtered_stages(&self) -> vantage_core::Result<Vec<Document>> {
        let (stored, joined) = self.match_stages().await?;
        let mut pipeline = self.pre_stages.clone();
        pipeline.extend(stored);
        if let Some(joined) = joined {
            pipeline.extend(self.join_stages());
            pipeline.push(joined);
        }
        Ok(pipeline)
    }

    /// The layout of a row read, given its two `$match` stages in whatever
    /// form `stage` renders documents to:
    /// [pre_stages.., stored $match, $sort, $skip, $limit, joins.., $project],
    /// with the joins (and the joined `$match`) moved ahead of the paging when
    /// a condition or the sort reads a joined column.
    pub(super) fn read_layout<S>(
        &self,
        stored: Option<S>,
        joined: Option<S>,
        stage: impl Fn(Document) -> S,
    ) -> Vec<S> {
        let mut out: Vec<S> = self.pre_stages.iter().cloned().map(&stage).collect();
        out.extend(stored);
        let joins = self.join_stages().into_iter().map(&stage);
        let page = self.page_stages().into_iter().map(&stage);
        if joined.is_some() || self.sort_uses_joins() {
            out.extend(joins);
            out.extend(joined);
            out.extend(page);
        } else {
            out.extend(page);
            out.extend(joins);
        }
        out.extend(self.projection_stage().map(&stage));
        out
    }

    /// [$sort, $skip, $limit], each only when set.
    fn page_stages(&self) -> Vec<Document> {
        let mut stages = Vec::new();
        if let Some(sort) = self.build_sort() {
            stages.push(doc! { "$sort": sort });
        }
        if let Some(skip) = self.skip.filter(|s| *s > 0) {
            stages.push(doc! { "$skip": skip });
        }
        if let Some(limit) = self.limit.filter(|l| *l > 0) {
            stages.push(doc! { "$limit": limit });
        }
        stages
    }

    fn projection_stage(&self) -> Option<Document> {
        self.build_projection()
            .map(|projection| doc! { "$project": projection })
    }

    /// Build a row-reading pipeline; see `read_layout` for the stage order.
    pub async fn as_read_pipeline(&self) -> vantage_core::Result<Vec<Document>> {
        let (stored, joined) = self.match_stages().await?;
        Ok(self.read_layout(stored, joined, |d| d))
    }

    /// Build a count aggregation pipeline: [pre_stages.., $match, $count],
    /// with the join stages only when the filter reads a joined column.
    pub async fn as_count_pipeline(&self) -> vantage_core::Result<Vec<Document>> {
        let mut pipeline = self.filtered_stages().await?;
        pipeline.push(doc! { "$count": "count" });
        Ok(pipeline)
    }

    /// Build a $group aggregation pipeline with an accumulator.
    /// `func` is e.g. "$sum", "$max", "$min". `field` is the column name.
    pub async fn as_aggregate_pipeline(
        &self,
        func: &str,
        field: &str,
    ) -> vantage_core::Result<Vec<Document>> {
        let field_ref = format!("${}", field);
        let mut pipeline = self.filtered_stages().await?;
        pipeline.push(doc! { "$group": { "_id": Bson::Null, "val": { func: field_ref } } });
        Ok(pipeline)
    }
}
