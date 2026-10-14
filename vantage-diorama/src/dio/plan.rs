//! Routing a query between the master and diorama.

use vantage_core::Result;
use vantage_vista::{Vista, VistaQuery};

use super::DioInner;

/// Where one query goes: the master narrowed by every clause it accepted, and
/// every clause it didn't. `refused` holds the clauses the master refused and
/// the ones diorama kept for itself (those on augmented columns), so it names
/// everything of the query a read from `narrowed` doesn't reflect. The Dio's
/// own residual is not in it; see [`DioInner::with_own_residual`].
pub(crate) struct Plan {
    pub narrowed: Vista,
    pub refused: VistaQuery,
}

impl DioInner {
    /// Route `query` against this Dio's master: the clauses on augmented
    /// columns stay with diorama, and the rest are offered to the master
    /// through [`Vista::with_query`]. Nothing is dropped; the caller decides
    /// what to do with [`Plan::refused`].
    ///
    /// A search is never offered to a master that already holds one: a shell
    /// replaces its search, so the read would escape the Dio's own. It lands
    /// in `refused` instead, to be evaluated over a complete set.
    ///
    /// Fails when the master rejects a clause outright (an unknown column),
    /// and when `query` holds native conditions.
    pub(crate) fn plan(&self, query: &VistaQuery) -> Result<Plan> {
        self.plan_on(self.master.clone(), query)
    }

    /// [`plan`](Self::plan) against `master`, which a derivation may have
    /// cleared of a clause the query replaces.
    pub(crate) fn plan_on(&self, master: Vista, query: &VistaQuery) -> Result<Plan> {
        let (mut offered, mut refused) = self.split_local(query.clone());
        if master.query().search.is_some() {
            refused.search = offered.search.take();
        }
        let (narrowed, from_master) = master.with_query(&offered)?;
        refused.conditions.extend(from_master.conditions);
        if refused.search.is_none() {
            refused.search = from_master.search;
        }
        if refused.order.is_none() {
            refused.order = from_master.order;
        }
        Ok(Plan { narrowed, refused })
    }

    /// `refused`, what the master left of `query`, with this Dio's own
    /// residual laid over it: every clause a read from the narrowed master
    /// doesn't reflect. The Dio's conditions join the list; its search fills
    /// the slot when `refused` has none, and its order when `query` brings
    /// none of its own (the page then arrives in the master's order, not the
    /// Dio's).
    ///
    /// A query holds one search, so when both are left over only the query's
    /// is named. A search in the result means "a search is left over", and
    /// evaluating the result over a complete set must always also apply the
    /// Dio's residual (`admitted_by_dio` in the table scenery).
    pub(crate) fn with_own_residual(
        &self,
        query: &VistaQuery,
        mut refused: VistaQuery,
    ) -> VistaQuery {
        refused
            .conditions
            .extend(self.residual.conditions.iter().cloned());
        if refused.search.is_none() {
            refused.search = self.residual.search.clone();
        }
        if query.order.is_none() && refused.order.is_none() {
            refused.order = self.residual.order.clone();
        }
        refused
    }

    /// Whether this Dio's residual narrows its rows: a condition or a search
    /// the master never applied.
    pub(crate) fn residual_narrows(&self) -> bool {
        !self.residual.conditions.is_empty() || self.residual.search.is_some()
    }

    /// Split `query` into what may be offered to the master and the clauses
    /// diorama keeps for itself: conditions on a declared augmented column or
    /// a dotted path under one, and an order on one. The master has never
    /// heard of those columns, and a shell may accept an unknown column only
    /// to fail when it fetches, or filter on a field it doesn't hold and
    /// return the wrong rows. A lift-all augmentation declares no columns, so
    /// its clauses are all offered.
    fn split_local(&self, mut query: VistaQuery) -> (VistaQuery, VistaQuery) {
        let mut kept = VistaQuery::new();
        let (local, offered) = std::mem::take(&mut query.conditions)
            .into_iter()
            .partition(|c| self.is_declared_augmented(&c.column));
        query.conditions = offered;
        kept.conditions = local;
        if query
            .order
            .as_ref()
            .is_some_and(|(column, _)| self.is_declared_augmented(column))
        {
            kept.order = query.order.take();
        }
        (query, kept)
    }
}

#[cfg(test)]
mod tests {
    use vantage_vista::mocks::MockShell;
    use vantage_vista::{Column, Vista, VistaMetadata, VistaQuery};

    use crate::{Cache, Lens};

    /// A master holding the Dio's search is never offered the view's, which it
    /// would take in place of the Dio's.
    #[tokio::test]
    async fn a_view_search_is_not_offered_over_a_pushed_dio_search() {
        let metadata = VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("team", "String"))
            .with_id_column("id");
        let master = Vista::new("teams", MockShell::new().with_metadata(metadata));
        let lens = Lens::builder()
            .cache(Cache::memory())
            .build()
            .expect("lens");
        let dio = lens.dio(master).build().await.expect("dio builds");
        let derived = dio
            .derive()
            .with_search("x")
            .build()
            .await
            .expect("derives");
        assert_eq!(derived.master().query().search.as_deref(), Some("x"));

        let plan = derived
            .inner
            .plan(&VistaQuery::new().with_search("y"))
            .expect("plans");
        assert_eq!(plan.refused.search.as_deref(), Some("y"));
        assert_eq!(
            plan.narrowed.query().search.as_deref(),
            Some("x"),
            "the narrowed master keeps the Dio's search"
        );
    }
}
