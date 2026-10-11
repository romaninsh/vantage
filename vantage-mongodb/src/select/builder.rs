//! Builder methods for MongoSelect — produce native MongoDB types
//! (filter Document, projection Document, sort Document, FindOptions).

use bson::Document;

use super::MongoSelect;

impl MongoSelect {
    /// Build the filter `Document` by resolving all conditions.
    pub async fn build_filter(&self) -> vantage_core::Result<Document> {
        crate::condition::resolve_conditions(self.conditions.iter()).await
    }

    /// Build the projection `Document`. Returns `None` if all fields requested.
    pub fn build_projection(&self) -> Option<Document> {
        if self.fields.is_empty() {
            return None;
        }
        let mut proj = Document::new();
        for f in &self.fields {
            proj.insert(f.as_str(), 1);
        }
        Some(proj)
    }

    /// Build the sort `Document`. Returns `None` if no ordering.
    pub fn build_sort(&self) -> Option<Document> {
        if self.sort.is_empty() {
            return None;
        }
        let mut doc = Document::new();
        for (field, dir) in &self.sort {
            doc.insert(field.as_str(), *dir);
        }
        Some(doc)
    }

    /// Build `mongodb::options::FindOptions` from the current state. A
    /// positive limit is also the batch size, so a page arrives in one
    /// round trip instead of the server's default first batch plus `getMore`s.
    pub fn build_find_options(&self) -> mongodb::options::FindOptions {
        let mut opts = mongodb::options::FindOptions::default();
        opts.projection = self.build_projection();
        opts.sort = self.build_sort();
        opts.limit = self.limit;
        opts.batch_size = self
            .limit
            .filter(|&n| n > 0)
            .map(|n| u32::try_from(n).unwrap_or(u32::MAX));
        opts.skip = self.skip.map(|s| s as u64);
        opts
    }
}

#[cfg(test)]
mod tests {
    use bson::doc;

    use super::*;
    use vantage_expressions::Selectable;

    #[test]
    fn test_build_projection() {
        let s = MongoSelect::new().with_field("name").with_field("price");
        let proj = s.build_projection().unwrap();
        assert_eq!(proj, doc! { "name": 1, "price": 1 });
    }

    #[test]
    fn test_build_projection_empty() {
        let s = MongoSelect::new();
        assert!(s.build_projection().is_none());
    }

    #[test]
    fn test_build_sort() {
        let mut s = MongoSelect::new();
        s.sort.push(("price".into(), 1));
        s.sort.push(("name".into(), -1));
        let sort = s.build_sort().unwrap();
        assert_eq!(sort, doc! { "price": 1, "name": -1 });
    }

    #[test]
    fn test_build_find_options() {
        let s = MongoSelect::new()
            .with_field("name")
            .with_limit(Some(5), Some(10));
        let opts = s.build_find_options();
        assert!(opts.projection.is_some());
        assert_eq!(opts.limit, Some(5));
        assert_eq!(opts.skip, Some(10));
    }

    #[test]
    fn test_build_find_options_batch_size_follows_limit() {
        let page = MongoSelect::new().with_limit(Some(50), None);
        assert_eq!(page.build_find_options().batch_size, Some(50));

        assert_eq!(MongoSelect::new().build_find_options().batch_size, None);

        let mut nonpositive = MongoSelect::new();
        for limit in [0, -3] {
            nonpositive.limit = Some(limit);
            assert_eq!(nonpositive.build_find_options().batch_size, None);
        }

        let mut huge = MongoSelect::new();
        huge.limit = Some(i64::MAX);
        assert_eq!(huge.build_find_options().batch_size, Some(u32::MAX));
    }

    #[tokio::test]
    async fn test_build_filter_empty() {
        let s = MongoSelect::new();
        let filter = s.build_filter().await.unwrap();
        assert_eq!(filter, doc! {});
    }

    #[tokio::test]
    async fn test_build_filter_single() {
        let s = MongoSelect::new().with_condition(doc! { "active": true });
        let filter = s.build_filter().await.unwrap();
        assert_eq!(filter, doc! { "active": true });
    }

    #[tokio::test]
    async fn test_build_filter_multiple() {
        let s = MongoSelect::new()
            .with_condition(doc! { "active": true })
            .with_condition(doc! { "price": { "$gt": 100 } });
        let filter = s.build_filter().await.unwrap();
        assert_eq!(
            filter,
            doc! { "$and": [{ "active": true }, { "price": { "$gt": 100 } }] }
        );
    }
}
