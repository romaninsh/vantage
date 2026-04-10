//! Aggregation pipeline builders for MongoSelect.

use bson::{Bson, Document, doc};

use super::MongoSelect;

impl MongoSelect {
    /// Build a count aggregation pipeline: [$match, $count].
    pub async fn as_count_pipeline(&self) -> vantage_core::Result<Vec<Document>> {
        let filter = self.build_filter().await?;
        let mut pipeline = Vec::new();
        if !filter.is_empty() {
            pipeline.push(doc! { "$match": filter });
        }
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
        let filter = self.build_filter().await?;
        let field_ref = format!("${}", field);
        let mut pipeline = Vec::new();
        if !filter.is_empty() {
            pipeline.push(doc! { "$match": filter });
        }
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
}
