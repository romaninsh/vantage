//! Reading a window straight from the master.

use std::ops::Range;

use vantage_core::Result;
use vantage_vista::{Page, VistaQuery};

use super::Dio;

/// One window read by [`Dio::fetch`], and the clauses of the query it
/// doesn't reflect.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Fetched {
    /// The rows the master returned, and the total when it reported one.
    pub page: Page,
    /// Every clause the page doesn't reflect: those of the query the master
    /// refused, those diorama keeps for itself (on augmented columns), and
    /// the Dio's own [`residual`](Dio::residual), its order included when the
    /// query brings none. Evaluating them over this
    /// one window would filter a slice and present it as the set, so they
    /// are handed back rather than applied.
    ///
    /// It holds one search: when both the query's and the Dio's are left
    /// over, it names the query's, and the Dio's residual search still
    /// applies.
    pub refused: VistaQuery,
}

impl Dio {
    /// Read the `window` of this Dio's master narrowed by `query`.
    ///
    /// The query is applied to a clone of the master, so the shared master
    /// is never changed and differently-sorted readers never race. What the
    /// master doesn't take comes back in [`Fetched::refused`].
    ///
    /// Fails when the master rejects a clause outright (an unknown column),
    /// or when the read itself fails.
    pub async fn fetch(&self, window: Range<usize>, query: &VistaQuery) -> Result<Fetched> {
        let plan = self.inner.plan(query)?;
        let page = plan
            .narrowed
            .fetch_window(window.start, window.len())
            .await?;
        Ok(Fetched {
            page,
            refused: self.inner.with_own_residual(query, plan.refused),
        })
    }
}
