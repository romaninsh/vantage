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

#[cfg(test)]
mod tests {
    use bson::doc;
    use vantage_expressions::Selectable;

    use super::*;

    #[tokio::test]
    async fn test_count_pipeline_empty() {
        let s = MongoSelect::new();
        let pipeline = s.as_count_pipeline().await.unwrap();
        assert_eq!(pipeline.len(), 1);
        assert_eq!(pipeline[0], doc! { "$count": "count" });
    }

    #[tokio::test]
    async fn test_count_pipeline_with_filter() {
        let s = MongoSelect::new().with_condition(doc! { "active": true });
        let pipeline = s.as_count_pipeline().await.unwrap();
        assert_eq!(pipeline.len(), 2);
        assert_eq!(pipeline[0], doc! { "$match": { "active": true } });
        assert_eq!(pipeline[1], doc! { "$count": "count" });
    }

    #[tokio::test]
    async fn test_aggregate_pipeline_sum() {
        let s = MongoSelect::new();
        let pipeline = s.as_aggregate_pipeline("$sum", "price").await.unwrap();
        assert_eq!(pipeline.len(), 1);
        assert_eq!(
            pipeline[0],
            doc! { "$group": { "_id": Bson::Null, "val": { "$sum": "$price" } } }
        );
    }

    #[tokio::test]
    async fn test_aggregate_pipeline_max_with_filter() {
        let s = MongoSelect::new().with_condition(doc! { "is_deleted": false });
        let pipeline = s.as_aggregate_pipeline("$max", "price").await.unwrap();
        assert_eq!(pipeline.len(), 2);
        assert_eq!(pipeline[0], doc! { "$match": { "is_deleted": false } });
        assert_eq!(
            pipeline[1],
            doc! { "$group": { "_id": Bson::Null, "val": { "$max": "$price" } } }
        );
    }

    #[tokio::test]
    async fn test_read_pipeline_orders_stages() {
        let mut s = MongoSelect::new()
            .with_condition(doc! { "n": { "$gt": 1 } })
            .with_limit(Some(5), Some(10));
        s.pre_stages = vec![doc! { "$group": { "_id": "$c" } }];
        s.sort.push(("n".into(), -1));
        assert_eq!(
            s.as_read_pipeline().await.unwrap(),
            vec![
                doc! { "$group": { "_id": "$c" } },
                doc! { "$match": { "n": { "$gt": 1 } } },
                doc! { "$sort": { "n": -1 } },
                doc! { "$skip": 10_i64 },
                doc! { "$limit": 5_i64 },
            ]
        );
    }

    fn client_name_join() -> crate::select::JoinColumn {
        crate::select::JoinColumn {
            name: "client.name".into(),
            hops: vec![crate::select::JoinHop {
                from: "clients".into(),
                local_field: "client_id".into(),
                foreign_field: "_id".into(),
            }],
            field: "name".into(),
        }
    }

    #[tokio::test]
    async fn test_read_pipeline_joins_before_match() {
        let mut s = MongoSelect::new()
            .with_condition(doc! { "__vc0": "Marty" })
            .with_limit(Some(5), None);
        s.pre_stages = vec![doc! { "$match": { "live": true } }];
        s.joins = vec![client_name_join()];
        s.sort.push(("__vc0".into(), 1));
        let pipeline = s.as_read_pipeline().await.unwrap();
        assert_eq!(pipeline[0], doc! { "$match": { "live": true } });
        assert!(pipeline[1].contains_key("$lookup"), "{pipeline:?}");
        assert_eq!(
            pipeline[3],
            doc! { "$addFields": { "__vc0": "$__vj0.name" } }
        );
        assert_eq!(pipeline[4], doc! { "$unset": ["__vj0"] });
        assert_eq!(
            pipeline[5..],
            [
                doc! { "$match": { "__vc0": "Marty" } },
                doc! { "$sort": { "__vc0": 1 } },
                doc! { "$limit": 5_i64 },
            ]
        );
    }

    #[tokio::test]
    async fn test_read_pipeline_joins_after_paging_when_nothing_reads_them() {
        let mut s = MongoSelect::new()
            .with_condition(doc! { "n": 1 })
            .with_condition(doc! { "__vc0": "Doc" })
            .with_limit(Some(5), None);
        s.joins = vec![client_name_join()];
        let pipeline = s.as_read_pipeline().await.unwrap();
        // The stored-field condition runs first; the joined one after the join.
        assert_eq!(pipeline[0], doc! { "$match": { "n": 1 } });
        assert!(pipeline[1].contains_key("$lookup"), "{pipeline:?}");
        assert_eq!(pipeline[5], doc! { "$match": { "__vc0": "Doc" } });
        assert_eq!(pipeline[6], doc! { "$limit": 5_i64 });

        s.conditions.pop();
        s.sort.push(("n".into(), 1));
        let pipeline = s.as_read_pipeline().await.unwrap();
        assert_eq!(
            pipeline[..3],
            [
                doc! { "$match": { "n": 1 } },
                doc! { "$sort": { "n": 1 } },
                doc! { "$limit": 5_i64 },
            ]
        );
        assert!(pipeline[3].contains_key("$lookup"), "{pipeline:?}");
        assert_eq!(pipeline.len(), 7);
    }

    #[tokio::test]
    async fn test_count_pipeline_joins_only_for_a_joined_filter() {
        let mut s = MongoSelect::new().with_condition(doc! { "n": 1 });
        s.joins = vec![client_name_join()];
        assert_eq!(s.as_count_pipeline().await.unwrap().len(), 2);

        let mut s = MongoSelect::new().with_condition(doc! { "__vc0": "Doc" });
        s.joins = vec![client_name_join()];
        let count = s.as_count_pipeline().await.unwrap();
        assert!(count[0].contains_key("$lookup"), "{count:?}");
        assert_eq!(count.len(), 6);
    }

    #[tokio::test]
    async fn test_pre_stages_lead_count_and_aggregate() {
        let mut s = MongoSelect::new();
        s.pre_stages = vec![doc! { "$unwind": "$items" }];
        let count = s.as_count_pipeline().await.unwrap();
        assert_eq!(count[0], doc! { "$unwind": "$items" });
        let sum = s.as_aggregate_pipeline("$sum", "qty").await.unwrap();
        assert_eq!(sum[0], doc! { "$unwind": "$items" });
        assert_eq!(sum.len(), 2);
    }
}
