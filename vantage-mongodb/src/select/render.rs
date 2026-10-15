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
