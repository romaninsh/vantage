//! Describing a reduction, so a driver can decide whether it can perform one.
//!
//! An aggregation is not a value — it is a **new set**. `SELECT count(*) …`
//! yields a one-row table, `GROUP BY` yields one row per group, and both are
//! ordinary sets you can then condition, order or count again. So the verb
//! this spec feeds ([`TableShell::aggregate_vista`](crate::TableShell::aggregate_vista))
//! returns a [`Vista`](crate::Vista), not a number, and every consumer keeps
//! the one shape it already knows.

/// What to reduce, how, and what to call the result.
///
/// **Conditions are not here.** Narrow the source vista first, then aggregate
/// it — the order SQL uses, where the filter belongs to the inner query and
/// the aggregate selects from the result. Narrowing already reports its own
/// failure (`add_condition_eq` returns `Err` on a driver that can't express a
/// term), so a caller knows to fall back *before* it asks for a reduction, and
/// no driver is ever handed a filter it will silently ignore.
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
            group_by: Vec::new(),
        }
    }

    /// Reduce `column` rather than whole rows.
    pub fn column(mut self, column: impl Into<String>) -> Self {
        self.column = Some(column.into());
        self
    }

    /// Emit one row per distinct value of `column`.
    pub fn group_by(mut self, column: impl Into<String>) -> Self {
        self.group_by.push(column.into());
        self
    }

    /// A stable, unique name for the set this spec derives from a source.
    ///
    /// `source_key` identifies the source **as narrowed** — pass
    /// [`Vista::index_key`](crate::Vista::index_key), which already renders a
    /// vista's conditions and sort in a canonical, order-independent form.
    /// That is what keeps two differently-filtered aggregates apart without
    /// this type knowing anything about conditions.
    ///
    /// Two things depend on the result. A derived Dio caches under it, so it
    /// must not collide with the source or with a differently-shaped aggregate
    /// over the same source. And the same question asked twice must produce
    /// the *same* key, so a page showing one number in two places shares one
    /// engine instead of computing it twice.
    ///
    /// `#` separates a source from its derivation: it has no other meaning
    /// here (dots are column paths, `/` already separates datasource from
    /// table), and it sorts each derived table next to its origin in a cache
    /// directory.
    ///
    /// ```
    /// # use vantage_vista::AggregateSpec;
    /// let spec = AggregateSpec::new("sum", "bytes").column("Size");
    /// assert_eq!(spec.cache_key("events|c:Event=opened|s:"),
    ///            "events|c:Event=opened|s:#sum(Size)");
    /// ```
    pub fn cache_key(&self, source_key: &str) -> String {
        let mut key = format!("{source_key}#{}", self.op);
        if let Some(column) = &self.column {
            key.push_str(&format!("({column})"));
        }
        if !self.group_by.is_empty() {
            // NOT sorted: grouping by (a, b) is a different table from
            // (b, a) — the key columns come out in this order.
            key.push_str(&format!("/by:{}", self.group_by.join(",")));
        }
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::MockShell;
    use crate::vista::Vista;
    use ciborium::Value as CborValue;

    fn text(s: &str) -> CborValue {
        CborValue::Text(s.to_string())
    }

    /// A narrowed source, keyed the way a caller is expected to key it.
    fn source_key(conditions: &[(String, CborValue)]) -> String {
        let vista = Vista::new("email_events", Box::new(MockShell::new()));
        vista.index_key(conditions, None)
    }

    #[test]
    fn a_bare_reduction_keys_on_its_op() {
        let key = AggregateSpec::new("count", "total").cache_key(&source_key(&[]));
        assert!(key.ends_with("#count"), "got {key}");
    }

    #[test]
    fn the_reduced_column_is_part_of_the_key() {
        let key = AggregateSpec::new("sum", "bytes")
            .column("Size")
            .cache_key(&source_key(&[]));
        assert!(key.ends_with("#sum(Size)"), "got {key}");
    }

    /// The point of keying off the narrowed source: two aggregates that differ
    /// ONLY in their source's conditions must not share a cache table, or one
    /// would serve the other's number.
    #[test]
    fn conditions_on_the_source_separate_two_aggregates() {
        let spec = AggregateSpec::new("count", "n");
        let opened = spec.cache_key(&source_key(&[("Event".into(), text("opened"))]));
        let failed = spec.cache_key(&source_key(&[("Event".into(), text("failed"))]));
        assert_ne!(opened, failed);
    }

    /// And the converse: the same question asked twice lands on ONE engine,
    /// however the source's terms were ordered — `index_key` sorts them.
    #[test]
    fn source_term_order_does_not_change_the_key() {
        let spec = AggregateSpec::new("count", "n");
        let a = spec.cache_key(&source_key(&[
            ("Event".into(), text("opened")),
            ("Ip".into(), text("1.2.3.4")),
        ]));
        let b = spec.cache_key(&source_key(&[
            ("Ip".into(), text("1.2.3.4")),
            ("Event".into(), text("opened")),
        ]));
        assert_eq!(a, b);
    }

    /// Grouping order IS significant — it decides the output column order.
    #[test]
    fn group_order_does_change_the_key() {
        let key = source_key(&[]);
        let a = AggregateSpec::new("count", "n")
            .group_by("a")
            .group_by("b")
            .cache_key(&key);
        let b = AggregateSpec::new("count", "n")
            .group_by("b")
            .group_by("a")
            .cache_key(&key);
        assert_ne!(a, b);
    }

    /// The alias names the output column, not the question — two aliases for
    /// the same reduction are the same computation and should share it.
    #[test]
    fn the_alias_does_not_affect_the_key() {
        let key = source_key(&[]);
        assert_eq!(
            AggregateSpec::new("count", "total").cache_key(&key),
            AggregateSpec::new("count", "observed").cache_key(&key),
        );
    }

    #[test]
    fn a_derived_key_cannot_collide_with_its_source() {
        let key = source_key(&[]);
        let derived = AggregateSpec::new("count", "total").cache_key(&key);
        assert!(derived.starts_with(&key));
        assert_ne!(derived, key);
    }
}
