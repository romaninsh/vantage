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
