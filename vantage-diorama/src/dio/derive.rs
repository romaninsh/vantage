//! Deriving a narrowed Dio from a built one, and following references.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{Vista, VistaQuery};

use super::Dio;

impl Dio {
    /// A new Dio narrowed by every clause of `query`, on the same Lens. This
    /// Dio is left as it was.
    ///
    /// The master takes what it can through [`Vista::with_query`]. What it
    /// refuses, and every clause on an augmented column, joins the derived
    /// Dio's [`residual`](Self::residual), which diorama evaluates over the
    /// rows it holds. A search or order in `query` replaces this Dio's,
    /// whether the master or the residual held it.
    ///
    /// The derived Dio has a cache table of its own, keyed by everything it
    /// is narrowed by, and shares this Dio's augmentations and live setting.
    /// Opening that table, and running its eager start, is why this is async.
    ///
    /// Fails when the master rejects a clause outright (an unknown column),
    /// and when `query` holds native conditions, which can't be re-applied
    /// from a `VistaQuery` nor evaluated in memory.
    pub async fn with_query(&self, query: &VistaQuery) -> Result<Dio> {
        // A search in `query` replaces this Dio's, so the master is offered
        // it with its own search cleared.
        let mut master = self.master().clone();
        if query.search.is_some() {
            master.set_search(None)?;
        }
        let super::Plan { narrowed, refused } = self.inner.plan_on(master, query)?;

        let mut residual = self.inner.residual.clone();
        residual.conditions.extend(refused.conditions);
        if query.search.is_some() {
            residual.search = refused.search;
        }
        if query.order.is_some() {
            residual.order = refused.order;
        }

        self.inner
            .lens
            .dio(narrowed)
            .derived(self.inner.augmentations.clone(), self.inner.live, residual)
            .build()
            .await
    }

    /// Follow `relation` from `row` to a new Dio over the related rows, on
    /// the same Lens. Its cache table is keyed by the narrowed target's
    /// query, so two parents' related rows never share one.
    ///
    /// The target inherits this Dio's live setting, but not its
    /// augmentations, which describe this Dio's table. A down target source
    /// doesn't fail here; it surfaces later as an empty, recovering scenery.
    /// The failures are structural: an undefined relation, or a row without
    /// the join column (its id, for most relations).
    pub async fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Dio> {
        let target = self.master().get_ref(relation, Some(row))?;
        self.related(target).await
    }

    /// Follow `relation` to its **bare** target — the relation's table with
    /// no row condition — as a new Dio on the same Lens. Where
    /// [`get_ref`](Self::get_ref) narrows the target to one row's related
    /// rows, this hands back every eligible row: what a reference picker
    /// lists, or where a new related row would be inserted.
    pub async fn get_ref_target(&self, relation: &str) -> Result<Dio> {
        let target = self.master().get_ref(relation, None)?;
        self.related(target).await
    }

    async fn related(&self, target: Vista) -> Result<Dio> {
        self.inner
            .lens
            .dio(target)
            .derived(None, self.inner.live, VistaQuery::new())
            .build()
            .await
    }
}
