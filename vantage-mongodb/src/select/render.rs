//! Human-readable preview rendering for MongoSelect.

use super::MongoSelect;
use super::join::filter_uses_joins;
use crate::condition::MongoCondition;

impl MongoSelect {
    /// Render a human-readable preview string (for debug/logging).
    pub fn preview(&self) -> String {
        let coll = self.collection.as_deref().unwrap_or("?");

        let filter_str = if self.conditions.is_empty() {
            "{}".to_string()
        } else {
            format!("<{} conditions>", self.conditions.len())
        };

        if !self.pre_stages.is_empty() || !self.joins.is_empty() {
            return self.preview_aggregate(coll, &filter_str);
        }

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

    /// The read pipeline (`as_read_pipeline`) spelled as
    /// `db.<coll>.aggregate([...])`, in the same stage order, with each
    /// filter summarised the same way as the `find` form. A deferred
    /// condition is not resolved here, so it counts as one on stored fields.
    fn preview_aggregate(&self, coll: &str, filter_str: &str) -> String {
        let joined = self
            .conditions
            .iter()
            .filter(
                |c| matches!(c, MongoCondition::Doc(d) if filter_uses_joins(d, self.joins.len())),
            )
            .count();
        let stored = self.conditions.len() - joined;
        let summary = |n: usize| {
            let filter = match n {
                n if n == self.conditions.len() => filter_str.to_string(),
                n => format!("<{n} conditions>"),
            };
            (n > 0).then(|| format!("{{ \"$match\": {filter} }}"))
        };
        let stages = self.read_layout(summary(stored), summary(joined), |d| d.to_string());
        format!("db.{}.aggregate([{}])", coll, stages.join(", "))
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
    fn test_preview_with_pre_stages_renders_aggregate() {
        let mut s = MongoSelect::new()
            .with_source("orders")
            .with_condition(doc! { "n": 1 })
            .with_limit(Some(5), None);
        s.pre_stages = vec![doc! { "$group": { "_id": "$c" } }];
        let p = s.preview();
        assert!(p.starts_with("db.orders.aggregate([{ \"$group\""), "{p}");
        assert!(p.contains("\"$match\": <1 conditions>"), "{p}");
        assert!(p.ends_with("{ \"$limit\": 5 }])"), "{p}");
    }

    #[test]
    fn test_preview_with_joins_shows_lookup() {
        let mut s = MongoSelect::new().with_source("orders");
        s.joins = vec![crate::select::JoinColumn {
            name: "client.name".into(),
            hops: vec![crate::select::JoinHop {
                from: "clients".into(),
                local_field: "client_id".into(),
                foreign_field: "_id".into(),
            }],
            field: "name".into(),
        }];
        let p = s.preview();
        assert!(p.starts_with("db.orders.aggregate([{ \"$lookup\""), "{p}");
        assert!(p.contains("\"__vc0\": \"$__vj0.name\""), "{p}");

        // A joined condition is matched after the joins, a stored one before.
        let s = s
            .with_condition(doc! { "n": 1 })
            .with_condition(doc! { "__vc0": "Doc" });
        let p = s.preview();
        assert!(
            p.starts_with("db.orders.aggregate([{ \"$match\": <1 conditions> }, { \"$lookup\""),
            "{p}"
        );
        assert!(
            p.ends_with("{ \"$unset\": [\"__vj0\"] }, { \"$match\": <1 conditions> }])"),
            "{p}"
        );
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
