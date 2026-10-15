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
//! [`vantage_vista::Catalog`], which resolves a vista by name for exactly the
//! same reason.

mod group;

use std::collections::HashMap;
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_vista::VistaMetadata;

use crate::aggregate::aggregation::{Aggregation, Reads, Rows};
use crate::aggregate::builtin::{Avg, Conditions, Count, Distinct, Max, Min, Sum, Where};

pub use group::GroupReduction;

/// A scalar aggregation whose concrete type isn't known until runtime.
///
/// [`Dio::value_scenery`](crate::Dio::value_scenery) is generic, which suits a
/// caller naming its aggregation in code; a caller reading one from
/// configuration needs a single type to hand over, and this is it.
pub type ScalarAggregation = Box<dyn Aggregation<Output = CborValue>>;

/// So a boxed aggregation can be passed anywhere a concrete one can, without
/// a `dyn` variant of every mount.
impl Aggregation for ScalarAggregation {
    type Output = CborValue;

    fn reads(&self) -> Reads {
        (**self).reads()
    }

    fn compute(&self, rows: &Rows, schema: &VistaMetadata) -> CborValue {
        (**self).compute(rows, schema)
    }
}

/// What a builder is handed: everything a declared aggregation can say about
/// itself besides its name.
#[non_exhaustive]
pub struct AggregationSpec {
    /// The column the reduction reads. `None` for reductions over whole rows.
    pub column: Option<String>,
    /// Rows the reduction is restricted to. Applied by the catalog, not by
    /// each builder — narrowing is orthogonal to reducing (see [`Where`]).
    pub conditions: Conditions,
}

impl AggregationSpec {
    pub fn new(column: Option<String>, conditions: Conditions) -> Self {
        Self { column, conditions }
    }

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
        catalog.register("count", |_, _| {
            Ok(Box::new(Count::rows()) as ScalarAggregation)
        });
        catalog.register("sum", |op, spec| {
            Ok(Box::new(Sum::of(spec.require_column(op)?)) as ScalarAggregation)
        });
        catalog.register("avg", |op, spec| {
            Ok(Box::new(Avg::of(spec.require_column(op)?)) as ScalarAggregation)
        });
        catalog.register("min", |op, spec| {
            Ok(Box::new(Min::of(spec.require_column(op)?)) as ScalarAggregation)
        });
        catalog.register("max", |op, spec| {
            Ok(Box::new(Max::of(spec.require_column(op)?)) as ScalarAggregation)
        });
        catalog.register("distinct", |op, spec| {
            Ok(Box::new(Distinct::of(spec.require_column(op)?)) as ScalarAggregation)
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

impl Default for AggregationCatalog {
    fn default() -> Self {
        Self::with_builtins()
    }
}
