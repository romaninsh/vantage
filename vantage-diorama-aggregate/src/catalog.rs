//! Building an aggregation from a *name*, so configuration can declare one.
//!
//! The typed constructors ([`Count`], [`Sum`], …) are the API for Rust callers
//! who know at compile time what they want. Configuration doesn't: a YAML file
//! says `op: sum`, and something has to turn that string into an
//! [`Aggregation`]. Doing it with a match at the call site means every consumer
//! grows the same six-arm match, and none of them can be extended with an
//! aggregation the crate doesn't ship.
//!
//! [`AggregationCatalog`] is that lookup — the same shape as
//! [`VistaCatalog`](vantage_vista_factory::VistaCatalog), which resolves a
//! vista by name for exactly the same reason.

use std::collections::HashMap;
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::{Result, error};

use vantage_types::Record;
use vantage_vista::Column;

use crate::aggregation::{Aggregation, Rows};
use crate::builtin::{Avg, Conditions, Count, Distinct, GroupBy, Max, Min, Reduce, Sum, Where};

/// A scalar aggregation whose concrete type isn't known until runtime.
///
/// [`AggregateLens::value`](crate::AggregateLens::value) is generic, which
/// suits a caller naming its aggregation in code; a caller reading one from
/// configuration needs a single type to hand over, and this is it.
pub type ScalarAggregation = Box<dyn Aggregation<Output = CborValue>>;

/// So a boxed aggregation can be passed anywhere a concrete one can — in
/// particular to `AggregateLens::value`, without it needing a `dyn` variant.
impl Aggregation for ScalarAggregation {
    type Output = CborValue;

    fn compute(&self, rows: &Rows) -> CborValue {
        (**self).compute(rows)
    }
}

/// What a builder is handed: everything a declared aggregation can say about
/// itself besides its name.
pub struct AggregationSpec {
    /// The column the reduction reads. `None` for reductions over whole rows.
    pub column: Option<String>,
    /// Rows the reduction is restricted to. Applied by the catalog, not by
    /// each builder — narrowing is orthogonal to reducing (see [`Where`]).
    pub conditions: Conditions,
}

impl AggregationSpec {
    /// The column, or a clear error naming the op that needed one.
    pub fn require_column(&self, op: &str) -> Result<String> {
        self.column.clone().ok_or_else(|| {
            error!(
                "aggregation requires a `column` to reduce",
                op = op.to_string()
            )
        })
    }
}

/// Builds one aggregation from its spec.
pub type AggregationBuilder =
    Arc<dyn Fn(&str, &AggregationSpec) -> Result<ScalarAggregation> + Send + Sync>;

/// Name → aggregation.
pub struct AggregationCatalog {
    builders: HashMap<String, AggregationBuilder>,
}

impl AggregationCatalog {
    /// An empty catalog. Prefer [`with_builtins`](Self::with_builtins) unless
    /// you specifically want to control which names exist.
    pub fn new() -> Self {
        Self {
            builders: HashMap::new(),
        }
    }

    /// Every reduction this crate ships, under its lowercase name: `count`,
    /// `sum`, `avg`, `min`, `max`, `distinct`.
    pub fn with_builtins() -> Self {
        let mut catalog = Self::new();
        catalog.register("count", |_, _| Ok(Box::new(Count::rows()) as ScalarAggregation));
        catalog.register("sum", |op, spec| {
            Ok(Box::new(Sum::new(spec.require_column(op)?)) as ScalarAggregation)
        });
        catalog.register("avg", |op, spec| {
            Ok(Box::new(Avg::new(spec.require_column(op)?)) as ScalarAggregation)
        });
        catalog.register("min", |op, spec| {
            Ok(Box::new(Min::new(spec.require_column(op)?)) as ScalarAggregation)
        });
        catalog.register("max", |op, spec| {
            Ok(Box::new(Max::new(spec.require_column(op)?)) as ScalarAggregation)
        });
        catalog.register("distinct", |op, spec| {
            Ok(Box::new(Distinct::new(spec.require_column(op)?)) as ScalarAggregation)
        });
        catalog
    }

    /// Register (or replace) the aggregation built for `name`. This is the
    /// extension point: an application with its own reduction makes it
    /// declarable by registering it here, with no change to this crate.
    pub fn register<F>(&mut self, name: impl Into<String>, builder: F)
    where
        F: Fn(&str, &AggregationSpec) -> Result<ScalarAggregation> + Send + Sync + 'static,
    {
        self.builders.insert(name.into(), Arc::new(builder));
    }

    /// The names this catalog can build, sorted — for error messages and for
    /// a config validator listing what's available.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.builders.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// Build the named aggregation, wrapping it in the spec's conditions.
    ///
    /// The narrowing is applied here rather than inside each builder, so every
    /// aggregation — including one an application registered itself — supports
    /// `where` without writing any code for it.
    pub fn build(&self, name: &str, spec: AggregationSpec) -> Result<ScalarAggregation> {
        let Some(builder) = self.builders.get(name) else {
            return Err(error!(
                "unknown aggregation",
                op = name.to_string(),
                known = self.names().join(", ")
            ));
        };
        let inner = builder(name, &spec)?;
        if spec.conditions.is_empty() {
            return Ok(inner);
        }
        Ok(Box::new(Where::new(spec.conditions, inner)) as ScalarAggregation)
    }
}

impl AggregationCatalog {
    /// Build a `group_by`: one row per distinct value of `key_column`, each
    /// carrying the key plus the named reduction over that group's rows.
    ///
    /// The per-group reduction is built through this same catalog, so a
    /// grouped `sum` and a top-level `sum` cannot disagree — same reduction,
    /// different row set — and an application-registered aggregation is
    /// groupable the moment it is registered.
    pub fn group_by(
        &self,
        key_column: impl Into<String>,
        op: &str,
        alias: impl Into<String>,
        spec: AggregationSpec,
    ) -> Result<GroupBy<Reduce<impl Fn(&CborValue, &[&Record<CborValue>]) -> Record<CborValue> + Send + Sync + 'static>>>
    {
        let alias = alias.into();
        // Built once, up front: a bad declaration must surface now, not once
        // per group on every recomputation.
        let reduction = self.build(op, spec)?;
        let out_name = alias.clone();
        let columns = vec![Column::new(alias, "f64")];
        Ok(GroupBy::column(
            key_column,
            Reduce::new(columns, move |_key, members| {
                let rows: Rows = members
                    .iter()
                    .enumerate()
                    .map(|(i, record)| (i.to_string(), (*record).clone()))
                    .collect();
                let mut out = Record::new();
                out.insert(out_name.clone(), reduction.compute(&rows));
                out
            }),
        ))
    }
}

impl Default for AggregationCatalog {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_types::Record;

    fn rows() -> Rows {
        let mut a = Record::new();
        a.insert("Event".into(), CborValue::Text("opened".into()));
        a.insert("Size".into(), CborValue::Integer(10.into()));
        let mut b = Record::new();
        b.insert("Event".into(), CborValue::Text("failed".into()));
        b.insert("Size".into(), CborValue::Integer(20.into()));
        [("a".to_string(), a), ("b".to_string(), b)]
            .into_iter()
            .collect()
    }

    fn spec(column: Option<&str>, conditions: Conditions) -> AggregationSpec {
        AggregationSpec {
            column: column.map(str::to_string),
            conditions,
        }
    }

    #[test]
    fn builds_a_named_reduction() {
        let catalog = AggregationCatalog::with_builtins();
        let agg = catalog
            .build("sum", spec(Some("Size"), Conditions::new()))
            .unwrap();
        assert_eq!(agg.compute(&rows()), CborValue::Integer(30.into()));
    }

    /// Conditions are applied by the catalog, so they work for every name
    /// including ones registered from outside this crate.
    #[test]
    fn applies_conditions_to_any_named_reduction() {
        let catalog = AggregationCatalog::with_builtins();
        let agg = catalog
            .build(
                "sum",
                spec(
                    Some("Size"),
                    Conditions::new().eq("Event", CborValue::Text("opened".into())),
                ),
            )
            .unwrap();
        assert_eq!(agg.compute(&rows()), CborValue::Integer(10.into()));
    }

    #[test]
    fn an_unknown_name_lists_what_is_available() {
        let catalog = AggregationCatalog::with_builtins();
        // `unwrap_err` would need `Debug` on a boxed trait object; match instead.
        let err = match catalog.build("median", spec(Some("Size"), Conditions::new())) {
            Ok(_) => panic!("an unknown op must not build"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("median"), "names the bad op: {err}");
        assert!(err.contains("distinct"), "lists the known ops: {err}");
    }

    /// `count` is the only builtin that reduces whole rows; the rest must say
    /// what they reduce, and saying so badly is an error, not a zero.
    #[test]
    fn a_reduction_without_its_column_fails_loudly() {
        let catalog = AggregationCatalog::with_builtins();
        assert!(catalog.build("sum", spec(None, Conditions::new())).is_err());
        assert!(catalog.build("count", spec(None, Conditions::new())).is_ok());
    }

    #[test]
    fn a_registered_aggregation_becomes_declarable() {
        let mut catalog = AggregationCatalog::with_builtins();
        catalog.register("always_seven", |_, _| {
            Ok(Box::new(Sevens) as ScalarAggregation)
        });
        let agg = catalog
            .build("always_seven", spec(None, Conditions::new()))
            .unwrap();
        assert_eq!(agg.compute(&rows()), CborValue::Integer(7.into()));
    }

    /// Grouping routes through the same catalog as a top-level reduction, so
    /// the groups must add up to the whole. If these ever diverge, two copies
    /// of "sum" have appeared.
    #[test]
    fn the_groups_agree_with_the_ungrouped_reduction() {
        let catalog = AggregationCatalog::with_builtins();
        let grouped = catalog
            .group_by(
                "Event",
                "sum",
                "bytes",
                spec(Some("Size"), Conditions::new()),
            )
            .unwrap()
            .compute(&rows());
        let total = catalog
            .build("sum", spec(Some("Size"), Conditions::new()))
            .unwrap()
            .compute(&rows());

        assert_eq!(grouped.len(), 2, "one row per distinct Event");
        let summed: i64 = grouped
            .rows
            .iter()
            .filter_map(|(_, row)| match row.get("bytes") {
                Some(CborValue::Integer(i)) => Some(i128::from(*i) as i64),
                _ => None,
            })
            .sum();
        assert_eq!(CborValue::Integer(summed.into()), total);
    }

    /// The key column comes back on every row — a breakdown you can't label
    /// isn't one.
    #[test]
    fn every_group_carries_its_key() {
        let catalog = AggregationCatalog::with_builtins();
        let grouped = catalog
            .group_by("Event", "count", "n", spec(None, Conditions::new()))
            .unwrap()
            .compute(&rows());
        for (_, row) in &grouped.rows {
            assert!(row.get("Event").is_some(), "row without its key: {row:?}");
            assert!(row.get("n").is_some());
        }
    }

    struct Sevens;
    impl Aggregation for Sevens {
        type Output = CborValue;
        fn compute(&self, _rows: &Rows) -> CborValue {
            CborValue::Integer(7.into())
        }
    }
}
