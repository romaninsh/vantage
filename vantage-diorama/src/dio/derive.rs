//! Deriving a narrowed Dio from a built one, and following references.

mod builder;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{Vista, VistaQuery};

use super::Dio;

pub use builder::DeriveBuilder;

impl Dio {
    /// Start a new Dio narrowed from this one. Its clauses are gathered on
    /// the [`DeriveBuilder`] and applied together by
    /// [`build`](DeriveBuilder::build); a search or order given there
    /// replaces this Dio's, whether its master or its residual held it.
    ///
    /// ```ignore
    /// let johns = dio
    ///     .derive()
    ///     .with_condition(owner_is_john)
    ///     .with_order("name", SortDirection::Ascending)
    ///     .build()
    ///     .await?;
    /// ```
    pub fn derive(&self) -> DeriveBuilder {
        DeriveBuilder::new(self.clone())
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
