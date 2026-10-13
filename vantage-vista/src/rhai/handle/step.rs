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
    Ref(String),
}

impl Step {
    /// Apply this step to `vista`; `ref_cap` caps the rows a `ref` step
    /// follows. `limit(n)` itself changes no Vista: terminals read at most
    /// [`Handle::row_limit`](super::Handle::row_limit) rows through
    /// `fetch_window(0, n)`.
    pub(super) fn apply(&self, mut vista: Vista, ref_cap: Option<usize>) -> Result<Vista> {
        match self {
            Step::Where { col, op, value } => vista.add_condition(col, *op, value.clone())?,
            Step::Sort { col, dir } => vista.set_order(Some((col, *dir)))?,
            Step::Search(text) => vista.set_search(Some(text))?,
            Step::Limit(_) => {}
            Step::Ref(rel) => return traverse(&vista, rel, ref_cap),
        }
        Ok(vista)
    }

    /// The cap on the rows a later `ref` follows, given `ref_cap` before this
    /// step: a `limit(n)` lowers it, a `ref` starts a new set without one.
    pub(super) fn ref_cap_after(&self, ref_cap: Option<usize>) -> Option<usize> {
        match self {
            Step::Limit(n) => Some(ref_cap.map_or(*n, |cap| cap.min(*n))),
            Step::Ref(_) => None,
            _ => ref_cap,
        }
    }
}
