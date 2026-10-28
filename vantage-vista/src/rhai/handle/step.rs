//! One narrowing step of a [`Handle`](super::Handle), and how it changes a
//! Vista.

use ciborium::Value as CborValue;
use vantage_core::Result;

use crate::rhai::traverse::traverse;
use crate::{FilterOp, sort::SortDirection, vista::Vista};

/// One narrowing verb a script called on a handle.
#[derive(Clone, Debug)]
pub(crate) enum Step {
    Where {
        col: String,
        op: FilterOp,
        value: CborValue,
    },
    Sort {
        col: String,
        dir: SortDirection,
    },
    Search(String),
    Limit(usize),
    Offset(usize),
    Ref(String),
}

/// The rows of a set a read takes by position: `offset` rows skipped, then
/// at most `limit`, as SQL's `LIMIT m OFFSET n`. The order of the `limit`
/// and `offset` steps doesn't matter; several `limit` steps keep the
/// smallest and several `offset` steps add up, so a later step only ever
/// narrows further.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RowWindow {
    pub offset: usize,
    pub limit: Option<usize>,
}

impl RowWindow {
    /// This window narrowed by `step`; steps other than `limit` and
    /// `offset` leave it as it is.
    pub(super) fn fold(self, step: &Step) -> RowWindow {
        match step {
            Step::Limit(n) => self.capped(Some(*n)),
            Step::Offset(n) => RowWindow {
                offset: self.offset.saturating_add(*n),
                ..self
            },
            _ => self,
        }
    }

    /// This window taking at most `cap` rows.
    pub(crate) fn capped(self, cap: Option<usize>) -> RowWindow {
        let limit = match (self.limit, cap) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        RowWindow { limit, ..self }
    }

    /// The rows this window takes from a set of `total`.
    pub(crate) fn count(self, total: i64) -> i64 {
        let skipped = i64::try_from(self.offset).unwrap_or(i64::MAX);
        let rest = total.saturating_sub(skipped).max(0);
        self.limit.map_or(rest, |n| rest.min(n as i64))
    }
}

impl Step {
    /// Apply this step to `vista`; `ref_window` is the window of rows a
    /// `ref` step follows. `limit(n)` and `offset(n)` change no Vista:
    /// terminals read the [`Handle::row_window`](super::Handle::row_window)
    /// through `fetch_window`.
    pub(super) fn apply(&self, mut vista: Vista, ref_window: RowWindow) -> Result<Vista> {
        match self {
            Step::Where { col, op, value } => vista.add_condition(col, *op, value.clone())?,
            Step::Sort { col, dir } => vista.set_order(Some((col, *dir)))?,
            Step::Search(text) => vista.set_search(Some(text))?,
            Step::Limit(_) | Step::Offset(_) => {}
            Step::Ref(rel) => return traverse(&vista, rel, ref_window),
        }
        Ok(vista)
    }

    /// The window of rows a later `ref` follows, given `ref_window` before
    /// this step: `limit` and `offset` narrow it, a `ref` starts a new set
    /// without one.
    pub(super) fn ref_window_after(&self, ref_window: RowWindow) -> RowWindow {
        match self {
            Step::Ref(_) => RowWindow::default(),
            step => ref_window.fold(step),
        }
    }

    /// The verb of a step that picks rows by position, which a write can't
    /// keep to.
    pub(super) fn positional_verb(&self) -> Option<&'static str> {
        match self {
            Step::Limit(_) => Some("limit"),
            Step::Offset(_) => Some("offset"),
            _ => None,
        }
    }
}
