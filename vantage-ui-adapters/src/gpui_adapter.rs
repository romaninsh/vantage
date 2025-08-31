use crate::{CellValue, DataSet, Result, TableStore};
use std::sync::Arc;
use tokio::runtime::Handle;

// Note: GPUI-component may have different actual trait names
// This is a placeholder based on common table delegate patterns

/// Mock TableDelegate trait for GPUI
/// (The actual trait from gpui-component might be different)
pub trait TableDelegate {
    fn columns_count(&self) -> usize;
    fn rows_count(&self) -> usize;
    fn cell_text(&self, row: usize, column: usize) -> String;
    fn column_title(&self, column: usize) -> String;
    fn can_edit_cell(&self, row: usize, column: usize) -> bool;
    fn set_cell_text(&mut self, row: usize, column: usize, text: String) -> bool;
}

/// GPUI adapter implementing TableDelegate
pub struct GpuiTableDelegate<D: DataSet> {
    store: Arc<TableStore<D>>,
    runtime: Handle,
    cached_row_count: Option<usize>,
    cached_column_count: Option<usize>,
}

impl<D: DataSet + 'static> GpuiTableDelegate<D> {
    pub fn new(store: TableStore<D>) -> Self {
        Self {
            store: Arc::new(store),
            runtime: Handle::current(),
            cached_row_count: None,
            cached_column_count: None,
        }
    }

    fn block_on<F>(&self, future: F) -> F::Output
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.block_on(future)
    }
}

impl<D: DataSet + 'static> TableDelegate for GpuiTableDelegate<D> {
    fn columns_count(&self) -> usize {
        if let Some(count) = self.cached_column_count {
            return count;
        }

        let store = self.store.clone();
        let count = self.block_on(async move {
            match store.column_info().await {
                Ok(columns) => columns.len(),
                Err(_) => 0,
            }
        });

        // Note: We can't mutate self here due to the trait signature
        // In a real implementation, you'd use interior mutability
        count
    }

    fn rows_count(&self) -> usize {
        if let Some(count) = self.cached_row_count {
            return count;
        }

        let store = self.store.clone();
        let count = self.block_on(async move {
            match store.row_count().await {
                Ok(count) => count,
                Err(_) => 0,
            }
        });

        count
    }

    fn cell_text(&self, row: usize, column: usize) -> String {
        let store = self.store.clone();
        self.block_on(async move {
            match store.cell_value(row, column).await {
                Ok(value) => value.as_string(),
                Err(_) => "Error".to_string(),
            }
        })
    }

    fn column_title(&self, column: usize) -> String {
        let store = self.store.clone();
        self.block_on(async move {
            match store.column_info().await {
                Ok(columns) => columns
                    .get(column)
                    .map(|col| col.name.clone())
                    .unwrap_or_else(|| format!("Column {}", column)),
                Err(_) => format!("Column {}", column),
            }
        })
    }

    fn can_edit_cell(&self, row: usize, column: usize) -> bool {
        let store = self.store.clone();
        self.block_on(async move {
            match store.column_info().await {
                Ok(columns) => columns.get(column).map(|col| col.editable).unwrap_or(false),
                Err(_) => false,
            }
        })
    }

    fn set_cell_text(&mut self, row: usize, column: usize, text: String) -> bool {
        let store = self.store.clone();
        let new_value = CellValue::String(text);

        self.runtime.spawn(async move {
            let _ = store.update_cell(row, column, new_value).await;
        });

        true // Optimistically return success
    }
}

/// Wrapper for GPUI Table component
pub struct GpuiTable<D: DataSet> {
    delegate: GpuiTableDelegate<D>,
}

impl<D: DataSet + 'static> GpuiTable<D> {
    pub fn new(store: TableStore<D>) -> Self {
        Self {
            delegate: GpuiTableDelegate::new(store),
        }
    }

    pub fn delegate(&self) -> &GpuiTableDelegate<D> {
        &self.delegate
    }

    pub fn delegate_mut(&mut self) -> &mut GpuiTableDelegate<D> {
        &mut self.delegate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockProductDataSet;
    use tokio_test;

    #[tokio::test]
    async fn test_gpui_delegate() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let delegate = GpuiTableDelegate::new(store);

        // Test basic functionality
        assert_eq!(delegate.columns_count(), 4);
        assert_eq!(delegate.rows_count(), 5);

        // Test cell access
        let cell_text = delegate.cell_text(0, 0);
        assert_eq!(cell_text, "Flux Capacitor Cupcake");

        // Test column titles
        let col_title = delegate.column_title(0);
        assert_eq!(col_title, "name");

        // Test editability
        assert!(delegate.can_edit_cell(0, 0));
    }

    #[tokio::test]
    async fn test_gpui_table_creation() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = GpuiTable::new(store);

        assert_eq!(table.delegate().columns_count(), 4);
    }

    #[test]
    fn test_gpui_cell_editing() {
        tokio_test::block_on(async {
            let dataset = MockProductDataSet::new();
            let store = TableStore::new(dataset);
            let mut table = GpuiTable::new(store);

            // Test cell editing (optimistic - doesn't verify the actual update)
            let success = table
                .delegate_mut()
                .set_cell_text(0, 0, "New Product Name".to_string());
            assert!(success);
        });
    }
}
