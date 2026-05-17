//! Sort direction at the `Vista` boundary.
//!
//! Vista mirrors `vantage-table`'s [`SortDirection`] but defines its own copy
//! because `vantage-vista` does not depend on `vantage-table`. Drivers
//! translate between the two with a trivial `match`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Ascending,
    Descending,
}
