//! Extension trait for filtering column collections

use indexmap::IndexMap;
use std::sync::Arc;

use crate::{ColumnFlag, ColumnLike};

/// Extension trait for filtering collections of columns
///
/// This trait provides convenient methods for filtering columns by flags,
/// allowing for fluent API usage.
///
/// # Examples
///
/// ```rust,ignore
/// use vantage_table::{ColumnCollectionExt, ColumnFlag};
///
/// // Get only visible columns (exclude hidden)
/// let visible = table.columns().exclude(ColumnFlag::Hidden);
///
/// // Get only mandatory columns
/// let mandatory = table.columns().only(ColumnFlag::Mandatory);
///
/// // Chain filters
/// let visible_mandatory = table.columns()
///     .exclude(ColumnFlag::Hidden)
///     .only(ColumnFlag::Mandatory);
/// ```
pub trait ColumnCollectionExt {
    /// Filter columns to only include those with the specified flag
    ///
    /// # Arguments
    ///
    /// * `flag` - The column flag to filter by
    ///
    /// # Returns
    ///
    /// A new Arc containing only columns that have the specified flag
    fn only(&self, flag: ColumnFlag) -> Arc<IndexMap<String, Arc<dyn ColumnLike>>>;

    /// Filter columns to exclude those with the specified flag
    ///
    /// # Arguments
    ///
    /// * `flag` - The column flag to filter out
    ///
    /// # Returns
    ///
    /// A new Arc containing only columns that do not have the specified flag
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Column;

    #[test]
    fn test_only_filter() {
        let mut columns: IndexMap<String, Arc<dyn ColumnLike>> = IndexMap::new();
        columns.insert(
            "id".to_string(),
            Arc::new(Column::new("id").with_flags(&[ColumnFlag::Mandatory])),
        );
        columns.insert(
            "name".to_string(),
            Arc::new(Column::new("name").with_flags(&[ColumnFlag::Mandatory])),
        );
        columns.insert(
            "description".to_string(),
            Arc::new(Column::new("description")),
        );

        let arc_columns = Arc::new(columns);
        let mandatory = arc_columns.only(ColumnFlag::Mandatory);

        assert_eq!(mandatory.len(), 2);
        assert!(mandatory.contains_key("id"));
        assert!(mandatory.contains_key("name"));
        assert!(!mandatory.contains_key("description"));
    }

    #[test]
    fn test_exclude_filter() {
        let mut columns: IndexMap<String, Arc<dyn ColumnLike>> = IndexMap::new();
        columns.insert(
            "id".to_string(),
            Arc::new(Column::new("id").with_flags(&[ColumnFlag::Hidden])),
        );
        columns.insert("name".to_string(), Arc::new(Column::new("name")));
        columns.insert(
            "password".to_string(),
            Arc::new(Column::new("password").with_flags(&[ColumnFlag::Hidden])),
        );

        let arc_columns = Arc::new(columns);
        let visible = arc_columns.exclude(ColumnFlag::Hidden);

        assert_eq!(visible.len(), 1);
        assert!(visible.contains_key("name"));
        assert!(!visible.contains_key("id"));
        assert!(!visible.contains_key("password"));
    }

    #[test]
    fn test_chaining_filters() {
        let mut columns: IndexMap<String, Arc<dyn ColumnLike>> = IndexMap::new();
        columns.insert(
            "id".to_string(),
            Arc::new(Column::new("id").with_flags(&[ColumnFlag::Mandatory])),
        );
        columns.insert(
            "name".to_string(),
            Arc::new(Column::new("name").with_flags(&[ColumnFlag::Mandatory, ColumnFlag::Hidden])),
        );
        columns.insert("email".to_string(), Arc::new(Column::new("email")));

        let arc_columns = Arc::new(columns);
        let visible_mandatory = arc_columns
            .exclude(ColumnFlag::Hidden)
            .only(ColumnFlag::Mandatory);

        assert_eq!(visible_mandatory.len(), 1);
        assert!(visible_mandatory.contains_key("id"));
        assert!(!visible_mandatory.contains_key("name")); // hidden
        assert!(!visible_mandatory.contains_key("email")); // not mandatory
    }
}
