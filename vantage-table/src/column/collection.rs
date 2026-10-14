use std::sync::Arc;

use indexmap::IndexMap;

use crate::{column::flags::ColumnFlag, traits::column_like::ColumnLike};

/// Filter a column collection by flag; the filters chain.
///
/// ```rust,ignore
/// let visible_mandatory = table.columns()
///     .exclude(ColumnFlag::Hidden)
///     .only(ColumnFlag::Mandatory);
/// ```
pub trait ColumnCollectionExt {
    /// A new collection of only the columns that have `flag`.
    fn only(&self, flag: ColumnFlag) -> Arc<IndexMap<String, Arc<dyn ColumnLike>>>;

    /// A new collection of only the columns that do not have `flag`.
    fn exclude(&self, flag: ColumnFlag) -> Arc<IndexMap<String, Arc<dyn ColumnLike>>>;
}

impl ColumnCollectionExt for Arc<IndexMap<String, Arc<dyn ColumnLike>>> {
    fn only(&self, flag: ColumnFlag) -> Arc<IndexMap<String, Arc<dyn ColumnLike>>> {
        let filtered: IndexMap<String, Arc<dyn ColumnLike>> = self
            .iter()
            .filter(|(_, col)| col.flags().contains(&flag))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Arc::new(filtered)
    }

    fn exclude(&self, flag: ColumnFlag) -> Arc<IndexMap<String, Arc<dyn ColumnLike>>> {
        let filtered: IndexMap<String, Arc<dyn ColumnLike>> = self
            .iter()
            .filter(|(_, col)| !col.flags().contains(&flag))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Arc::new(filtered)
    }
}
