//! Human-readable preview rendering for MongoSelect.

use super::MongoSelect;

impl MongoSelect {
    /// Render a human-readable preview string (for debug/logging).
    pub fn preview(&self) -> String {
        let coll = self.collection.as_deref().unwrap_or("?");

        let filter_str = if self.conditions.is_empty() {
            "{}".to_string()
        } else {
            format!("<{} conditions>", self.conditions.len())
        };

        let mut parts = vec![format!("db.{}.find({})", coll, filter_str)];

        if let Some(proj) = self.build_projection() {
            parts.push(format!(".projection({})", proj));
        }
        if let Some(sort) = self.build_sort() {
            parts.push(format!(".sort({})", sort));
        }
        if let Some(skip) = self.skip {
            parts.push(format!(".skip({})", skip));
        }
        if let Some(limit) = self.limit {
            parts.push(format!(".limit({})", limit));
        }

        parts.join("")
    }
}

#[cfg(test)]
mod tests {
    use bson::doc;
    use vantage_expressions::Selectable;

    use super::*;

    #[test]
    fn test_empty_preview() {
        let s = MongoSelect::new();
        assert_eq!(s.preview(), "db.?.find({})");
    }

    #[test]
    fn test_preview_with_source_and_fields() {
        let s = MongoSelect::new()
            .with_source("product")
            .with_field("name")
            .with_field("price");
        let p = s.preview();
        assert!(p.starts_with("db.product.find({})"));
        assert!(p.contains("projection"));
    }

    #[test]
    fn test_preview_with_conditions() {
        let s = MongoSelect::new()
            .with_source("product")
            .with_condition(doc! { "price": { "$gt": 100 } });
        assert!(s.preview().contains("<1 conditions>"));
    }

    #[test]
    fn test_preview_with_limit_skip() {
        let s = MongoSelect::new()
            .with_source("x")
            .with_limit(Some(10), Some(20));
        let p = s.preview();
        assert!(p.contains(".skip(20)"));
        assert!(p.contains(".limit(10)"));
    }

    #[test]
    fn test_preview_with_sort() {
        use vantage_expressions::Order;
        let s = MongoSelect::new()
            .with_source("x")
            .with_order(doc! { "price": 1 }, Order::Asc);
        assert!(s.preview().contains(".sort("));
    }
}
