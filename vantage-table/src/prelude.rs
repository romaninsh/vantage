// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! The commonly used types and traits of vantage-table, for `use vantage_table::prelude::*;`.

pub use crate::table::Table;
pub use crate::table::{Hook, HookReturn, IdGenerator, Phase, Timestamps};

pub use crate::column::collection::ColumnCollectionExt;
pub use crate::column::core::Column;
pub use crate::column::flags::ColumnFlag;

pub use crate::active_entity_ext::GetRefExt;
pub use crate::traits::column_like::ColumnLike;
pub use crate::traits::table_source::TableSource;

pub use crate::sorting::{OrderBy, SortDirection};
pub use crate::table::sorting::OrderByExt;

pub use crate::pagination::Pagination;

pub use crate::conditions::ConditionHandle;

pub use crate::cbor_ext::CborValueExt;

#[cfg(any(test, feature = "mocks"))]
pub use crate::mocks::mock_table_source::MockTableSource;
