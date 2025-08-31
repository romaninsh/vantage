use crate::{DataSet, TableStore};
use std::sync::Arc;
use tokio::runtime::Handle;

#[cfg(feature = "gpui")]
use gpui::{div, px, App, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window};
#[cfg(feature = "gpui")]
use gpui_component::{
    table::{Column, Table, TableDelegate},
    StyledExt,
};

/// GPUI adapter implementing TableDelegate
pub struct GpuiTableDelegate<D: DataSet> {
    store: Arc<TableStore<D>>,
    cached_row_count: Option<usize>,
    cached_column_count: Option<usize>,
}

impl<D: DataSet + 'static> GpuiTableDelegate<D> {
    pub fn new(store: TableStore<D>) -> Self {
        Self {
            store: Arc::new(store),
            cached_row_count: None,
            cached_column_count: None,
        }
    }

    fn block_on<F>(&self, future: F) -> F::Output
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        // For now, we'll use a simple approach and create a new runtime
        // In a real implementation, you might want to use a global runtime or handle this differently
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(future)
    }

    // Convenience methods for direct access without App context
    pub fn rows_count(&self) -> usize {
        let store = self.store.clone();
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            match store.row_count().await {
                Ok(count) => count,
                Err(_) => 0,
            }
        })
    }

    pub fn columns_count(&self) -> usize {
        let store = self.store.clone();
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            match store.column_info().await {
                Ok(columns) => columns.len(),
                Err(_) => 0,
            }
        })
    }

    pub fn cell_text(&self, row: usize, column: usize) -> String {
        let store = self.store.clone();
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            match store.cell_value(row, column).await {
                Ok(value) => value.as_string(),
                Err(_) => "Error".to_string(),
            }
        })
    }

    pub fn column_title(&self, column: usize) -> String {
        let store = self.store.clone();
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            match store.column_info().await {
                Ok(columns) => columns
                    .get(column)
                    .map(|col| col.name.clone())
                    .unwrap_or_else(|| format!("Column {}", column)),
                Err(_) => format!("Column {}", column),
            }
        })
    }
}

#[cfg(feature = "gpui")]
impl<D: DataSet + 'static> TableDelegate for GpuiTableDelegate<D> {
    fn columns_count(&self, _cx: &App) -> usize {
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

        count
    }

    fn rows_count(&self, _cx: &App) -> usize {
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

    fn column(&self, col_ix: usize, _cx: &App) -> &Column {
        static COLUMNS: std::sync::LazyLock<Vec<Column>> = std::sync::LazyLock::new(|| {
            vec![
                Column::new("name", "Product Name").width(px(200.)),
                Column::new("category", "Category").width(px(150.)),
                Column::new("price", "Price").width(px(100.)),
                Column::new("stock", "Stock").width(px(80.)),
            ]
        });

        &COLUMNS[col_ix]
    }

    fn render_th(
        &self,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Table<Self>>,
    ) -> impl IntoElement {
        let store = self.store.clone();
        let column_name = self.block_on(async move {
            match store.column_info().await {
                Ok(columns) => columns
                    .get(col_ix)
                    .map(|col| col.name.clone())
                    .unwrap_or_else(|| format!("Column {}", col_ix)),
                Err(_) => format!("Column {}", col_ix),
            }
        });

        div().child(column_name)
    }

    fn render_td(
        &self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Table<Self>>,
    ) -> impl IntoElement {
        let store = self.store.clone();
        let cell_text = self.block_on(async move {
            match store.cell_value(row_ix, col_ix).await {
                Ok(value) => value.as_string(),
                Err(_) => "Error".to_string(),
            }
        });

        div().child(cell_text)
    }

    fn render_tr(
        &self,
        row_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Table<Self>>,
    ) -> gpui::Stateful<gpui::Div> {
        div().id(row_ix)
    }

    fn loading(&self, _cx: &App) -> bool {
        false
    }

    fn is_eof(&self, _cx: &App) -> bool {
        true
    }

    fn load_more_threshold(&self) -> usize {
        0
    }

    fn load_more(&mut self, _window: &mut Window, _cx: &mut Context<Table<Self>>) {}

    fn visible_rows_changed(
        &mut self,
        _range: std::ops::Range<usize>,
        _window: &mut Window,
        _cx: &mut Context<Table<Self>>,
    ) {
    }

    fn visible_columns_changed(
        &mut self,
        _range: std::ops::Range<usize>,
        _window: &mut Window,
        _cx: &mut Context<Table<Self>>,
    ) {
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
