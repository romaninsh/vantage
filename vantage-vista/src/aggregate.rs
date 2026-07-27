//! Describing a reduction, so a driver can decide whether it can perform one.
//!
//! An aggregation is not a value — it is a **new set**. `SELECT count(*) …`
//! yields a one-row table, `GROUP BY` yields one row per group, and both are
//! ordinary sets you can then condition, order or count again. So the verb
//! this spec feeds ([`TableShell::aggregate_vista`](crate::TableShell::aggregate_vista))
//! returns a [`Vista`](crate::Vista), not a number, and every consumer keeps
//! the one shape it already knows.

use ciborium::Value as CborValue;

/// What to reduce, how, and what to call the result.
///
/// Conditions live here rather than being applied to the source vista
/// beforehand so a driver can accept or refuse the request **as a unit**. A
/// driver that can sum but cannot express one of the terms must not quietly
/// sum the unfiltered set; given the whole question it can answer `None` and
/// let the caller reduce locally instead.
#[derive(Debug, Clone, PartialEq)]
pub struct AggregateSpec {
    /// The reduction: `count`, `sum`, `avg`, `min`, `max`, `distinct`, or
    /// whatever else a driver understands. A driver that doesn't recognise
    /// the name answers `None`.
    pub op: String,
    /// Column being reduced. `None` for reductions over whole rows (`count`).
    pub column: Option<String>,
    /// Column name the result is published under — the single column of a
    /// scalar aggregate's single row.
    pub alias: String,
    /// Equality terms, ANDed, restricting the rows reduced.
    pub conditions: Vec<(String, CborValue)>,
    /// Group keys. Empty means one row out; otherwise one row per distinct
    /// combination, carrying the key columns alongside `alias`.
    pub group_by: Vec<String>,
}

impl AggregateSpec {
    /// A scalar reduction over whole rows, e.g. `count`.
    pub fn new(op: impl Into<String>, alias: impl Into<String>) -> Self {
        Self {
            op: op.into(),
            column: None,
            alias: alias.into(),
            conditions: Vec::new(),
            group_by: Vec::new(),
        }
    }

    /// Reduce `column` rather than whole rows.
    pub fn column(mut self, column: impl Into<String>) -> Self {
        self.column = Some(column.into());
        self
    }

    /// Restrict to rows where `field == value`.
    pub fn condition(mut self, field: impl Into<String>, value: CborValue) -> Self {
        self.conditions.push((field.into(), value));
        self
    }

    /// Emit one row per distinct value of `column`.
    pub fn group_by(mut self, column: impl Into<String>) -> Self {
        self.group_by.push(column.into());
        self
    }

    /// A stable, unique name for the set this spec derives from `table`.
    ///
    /// Two things depend on it. A derived Dio caches under this name, so it
    /// must not collide with the source table or with a differently-shaped
    /// aggregate over the same source. And two identical aggregates must
    /// produce the *same* key, so a page showing one number twice shares one
    /// engine instead of computing it twice.
    ///
    /// `#` separates the source from its derivation: it has no other meaning
    /// in this vocabulary (dots are column paths, `/` already separates
    /// datasource from table), and it sorts each derived table next to the
    /// one it came from in a cache directory.
    ///
    /// ```
    /// # use vantage_vista::AggregateSpec;
    /// # use ciborium::Value as CborValue;
    /// let spec = AggregateSpec::new("count", "opened")
    ///     .condition("Event", CborValue::Text("opened".into()));
    /// assert_eq!(spec.cache_key("email_events"), "email_events#count[Event=opened]");
    /// ```
    pub fn cache_key(&self, table: &str) -> String {
        let mut key = format!("{table}#{}", self.op);
        if let Some(column) = &self.column {
            key.push_str(&format!("({column})"));
        }
        if !self.conditions.is_empty() {
            // Sorted, so two specs that differ only in the order the terms
            // were added still share a cache table and an engine.
            let mut terms: Vec<String> = self
                .conditions
                .iter()
                .map(|(field, value)| format!("{field}={}", scalar(value)))
                .collect();
            terms.sort();
            key.push_str(&format!("[{}]", terms.join(",")));
        }
        if !self.group_by.is_empty() {
            // NOT sorted: grouping by (a, b) is a different table from
            // (b, a) — the key columns come out in this order.
            key.push_str(&format!("/by:{}", self.group_by.join(",")));
        }
        key
    }
}

/// Render a condition value for the cache key. Non-scalars get a shape-stable
/// debug form — they're rare in an equality term, and a key only has to be
/// stable and unique, not readable.
fn scalar(value: &CborValue) -> String {
    match value {
        CborValue::Text(s) => s.clone(),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Float(f) => f.to_string(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Null => "null".to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> CborValue {
        CborValue::Text(s.to_string())
    }

    #[test]
    fn a_bare_reduction_keys_on_its_op() {
        assert_eq!(
            AggregateSpec::new("count", "total").cache_key("email_events"),
            "email_events#count"
        );
    }

    #[test]
    fn the_reduced_column_is_part_of_the_key() {
        assert_eq!(
            AggregateSpec::new("sum", "bytes")
                .column("Size")
                .cache_key("email_events"),
            "email_events#sum(Size)"
        );
    }

    /// The point of the key: two differently-conditioned aggregates over one
    /// source must not share a cache table, or one would serve the other's
    /// number.
    #[test]
    fn conditions_separate_two_aggregates_over_one_source() {
        let opened = AggregateSpec::new("count", "opened").condition("Event", text("opened"));
        let failed = AggregateSpec::new("count", "failed").condition("Event", text("failed"));
        assert_ne!(
            opened.cache_key("email_events"),
            failed.cache_key("email_events")
        );
    }

    /// And the converse: the same question asked twice must land on ONE
    /// engine, however the terms were ordered.
    #[test]
    fn term_order_does_not_change_the_key() {
        let a = AggregateSpec::new("count", "x")
            .condition("Event", text("opened"))
            .condition("Ip", text("1.2.3.4"));
        let b = AggregateSpec::new("count", "x")
            .condition("Ip", text("1.2.3.4"))
            .condition("Event", text("opened"));
        assert_eq!(a.cache_key("t"), b.cache_key("t"));
    }

    /// Grouping order IS significant — it decides the output column order.
    #[test]
    fn group_order_does_change_the_key() {
        let a = AggregateSpec::new("count", "n").group_by("a").group_by("b");
        let b = AggregateSpec::new("count", "n").group_by("b").group_by("a");
        assert_ne!(a.cache_key("t"), b.cache_key("t"));
    }

    /// The alias names the output column, not the question — two aliases for
    /// the same reduction are the same computation and should share it.
    #[test]
    fn the_alias_does_not_affect_the_key() {
        let a = AggregateSpec::new("count", "total");
        let b = AggregateSpec::new("count", "observed");
        assert_eq!(a.cache_key("t"), b.cache_key("t"));
    }

    #[test]
    fn a_derived_key_cannot_collide_with_a_plain_table() {
        let key = AggregateSpec::new("count", "total").cache_key("email_events");
        assert!(key.starts_with("email_events#"));
        assert_ne!(key, "email_events");
    }
}
