//! Gathering the clauses of a derivation, then building its one Dio.

use vantage_core::Result;
use vantage_vista::{Condition, SortDirection, VistaQuery};

use crate::dio::{Dio, Plan};

/// The clauses of a Dio to derive, gathered by [`Dio::derive`]. Each method
/// only records its clause; [`build`](Self::build) routes the whole query
/// once and builds one Dio from it, so every error surfaces there.
#[must_use = "a DeriveBuilder does nothing until `build().await`"]
pub struct DeriveBuilder {
    parent: Dio,
    query: VistaQuery,
}

impl DeriveBuilder {
    pub(crate) fn new(parent: Dio) -> Self {
        Self {
            parent,
            query: VistaQuery::new(),
        }
    }

    /// Narrow by `condition`, beside every condition the parent holds.
    pub fn with_condition(mut self, condition: Condition) -> Self {
        self.query.conditions.push(condition);
        self
    }

    /// Order by `column`, in place of the parent's order.
    pub fn with_order(mut self, column: impl Into<String>, dir: SortDirection) -> Self {
        self.query.order = Some((column.into(), dir));
        self
    }

    /// Search for `text`, in place of the parent's search.
    pub fn with_search(mut self, text: impl Into<String>) -> Self {
        self.query.search = Some(text.into());
        self
    }

    /// Every clause of `query`: its conditions join those gathered so far,
    /// and its search and order, when it has them, replace theirs.
    pub fn with_query(mut self, query: &VistaQuery) -> Self {
        self.query.conditions.extend(query.conditions.iter().cloned());
        if query.search.is_some() {
            self.query.search = query.search.clone();
        }
        if query.order.is_some() {
            self.query.order = query.order.clone();
        }
        self
    }

    /// A new Dio narrowed by every gathered clause, on the parent's Lens.
    /// The parent is left as it was.
    ///
    /// The master takes what it can through
    /// [`Vista::with_query`](vantage_vista::Vista::with_query). What it
    /// refuses, and every clause on an augmented column, joins the derived
    /// Dio's [`residual`](Dio::residual), which diorama evaluates over the
    /// rows it holds.
    ///
    /// The derived Dio has a cache table of its own, keyed by everything it
    /// is narrowed by, and shares the parent's augmentations and live
    /// setting, so the Lens hands two derivations of one query the same Dio.
    /// Opening that table, and running its eager start, is why this is async.
    ///
    /// Fails when the master rejects a clause outright (an unknown column),
    /// and when the query holds native conditions, which can't be re-applied
    /// from a `VistaQuery` nor evaluated in memory.
    pub async fn build(self) -> Result<Dio> {
        let Self { parent, query } = self;
        let inner = &parent.inner;
        // A search in the query replaces the parent's, so the master is
        // offered it with its own search cleared.
        let mut master = inner.master.clone();
        if query.search.is_some() {
            master.set_search(None)?;
        }
        let Plan { narrowed, refused } = inner.plan_on(master, &query)?;

        let mut residual = inner.residual.clone();
        residual.conditions.extend(refused.conditions);
        if query.search.is_some() {
            residual.search = refused.search;
        }
        if query.order.is_some() {
            residual.order = refused.order;
        }

        inner
            .lens
            .dio(narrowed)
            .derived(inner.augmentations.clone(), inner.live, residual)
            .build()
            .await
    }
}
