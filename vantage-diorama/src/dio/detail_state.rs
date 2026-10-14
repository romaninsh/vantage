//! What the detail pass last concluded about each row.
//!
//! The sweep can only tell a settled row from the record, at a cache read per
//! row. On a view whose query forces whole-index hydration that is a read of
//! every listed row on every pass. The state records the answer instead, so
//! a settled row costs one lookup. It memoizes the sweep's whole skip rule
//! (`Complete` and no augment gap), not the status alone: a cache warmed by
//! plain inserts stores rows `Complete` without a detail pass.
//!
//! A refresh demotes changed rows to `Incomplete` to make the pass fetch them
//! again, so every refresh forgets `Filled` and `Failed`. `Empty` survives a
//! scheduled one: a timed re-pull of the master is no evidence the detail
//! source gained a record, and re-probing every empty row on each tick keeps
//! the sweep from ever finishing. A requested refresh forgets it too.

use super::DioInner;

/// The detail pass's last answer for one row. A row holds at most one; a row
/// with none hasn't settled since the last refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailState {
    /// Hydrated, every augmented column filled.
    Filled,
    /// Hydrated, and the detail source has no record to fill it: the gap is
    /// the row's real value.
    Empty,
    /// The last detail fetch failed. The sweep retries it; a clause on an
    /// augmented value doesn't judge it.
    Failed,
}

impl DioInner {
    pub(crate) fn detail_state(&self, id: &str) -> Option<DetailState> {
        self.detail_state.lock().unwrap().get(id).copied()
    }

    pub(crate) fn set_detail_state(&self, id: &str, state: DetailState) {
        self.detail_state
            .lock()
            .unwrap()
            .insert(id.to_string(), state);
    }

    /// Forget `id`'s `Filled`: the list pass wrote its list row back over the
    /// detail, so the row has to be read again.
    pub(crate) fn forget_filled(&self, id: &str) {
        let mut states = self.detail_state.lock().unwrap();
        if states.get(id) == Some(&DetailState::Filled) {
            states.remove(id);
        }
    }

    /// Forget every answer, or every one but `Empty` when `keep_empty` (a
    /// scheduled refresh).
    pub(crate) fn clear_detail_states(&self, keep_empty: bool) {
        let mut states = self.detail_state.lock().unwrap();
        if keep_empty {
            states.retain(|_, state| *state == DetailState::Empty);
        } else {
            states.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use vantage_vista::mocks::MockShell;
    use vantage_vista::{Column, Vista, VistaMetadata};

    use super::DetailState::{Empty, Failed, Filled};
    use crate::{Cache, Lens};

    #[tokio::test]
    async fn an_id_holds_one_state() {
        let metadata = VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_id_column("id");
        let lens = Lens::builder()
            .cache(Cache::memory())
            .build()
            .expect("lens builds");
        let dio = lens
            .dio(Vista::new("items", MockShell::new().with_metadata(metadata)))
            .build()
            .await
            .expect("dio builds");
        let inner = &dio.inner;

        inner.set_detail_state("a", Filled);
        inner.set_detail_state("a", Failed);
        assert_eq!(inner.detail_state("a"), Some(Failed));
        assert_eq!(inner.detail_state.lock().unwrap().len(), 1);

        inner.set_detail_state("b", Empty);
        inner.set_detail_state("c", Filled);
        inner.clear_detail_states(true);
        assert_eq!(inner.detail_state("a"), None);
        assert_eq!(inner.detail_state("b"), Some(Empty));
        assert_eq!(inner.detail_state("c"), None);

        inner.clear_detail_states(false);
        assert_eq!(inner.detail_state("b"), None);
    }
}
