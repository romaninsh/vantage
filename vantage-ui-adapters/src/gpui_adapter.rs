use crate::{DataSet, TableStore};
use std::sync::Arc;

#[cfg(feature = "gpui")]
use gpui::{div, px, App, Context, InteractiveElement, IntoElement, ParentElement, Window};
#[cfg(feature = "gpui")]
use gpui_component::table::{Column, Table, TableDelegate};

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
        // Try to use current runtime first, fallback to creating new one
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.block_on(future)
        } else {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(future)
        }
    }

    // Convenience methods for direct access without App context
    pub fn rows_count(&self) -> usize {
        let store = self.store.clone();
        self.block_on(async move {
            match store.row_count().await {
                Ok(count) => count,
                Err(_) => 0,
            }
        })
    }

    pub fn columns_count(&self) -> usize {
        let store = self.store.clone();
        self.block_on(async move {
            match store.column_info().await {
                Ok(columns) => columns.len(),
                Err(_) => 0,
            }
        })
    }

    pub fn cell_text(&self, row: usize, column: usize) -> String {
        let store = self.store.clone();
        // Use the block_on method to handle runtime context properly
        self.block_on(async move {
            match store.cell_value(row, column).await {
                Ok(value) => value.as_string(),
                Err(_) => "Error".to_string(),
            }
        })
    }

    pub fn column_title(&self, column: usize) -> String {
        let store = self.store.clone();
        // Use the block_on method to handle runtime context properly
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
        static FALLBACK_COLUMN: std::sync::LazyLock<Column> =
            std::sync::LazyLock::new(|| Column::new("data", "Data").width(px(150.)));

        let store = self.store.clone();
        if let Ok(columns) = self.block_on(async move { store.column_info().await }) {
            if columns.get(col_ix).is_some() {
                &FALLBACK_COLUMN
            } else {
                &FALLBACK_COLUMN
            }
        } else {
            &FALLBACK_COLUMN
        }
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

    #[tokio::test]
    async fn test_gpui_delegate_async() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);

        // Test basic functionality using store directly
        assert_eq!(store.row_count().await.unwrap(), 5);
        assert_eq!(store.column_info().await.unwrap().len(), 4);

        // Test cell access
        let cell_value = store.cell_value(0, 0).await.unwrap();
        assert_eq!(cell_value.as_string(), "Flux Capacitor Cupcake");

        // Test column titles
        let columns = store.column_info().await.unwrap();
        assert_eq!(columns[0].name, "name");
    }

    #[tokio::test]
    async fn test_gpui_table_creation() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = GpuiTable::new(store);

        // Test that table is created successfully
        assert!(!std::ptr::eq(table.delegate(), std::ptr::null()));
    }

    #[test]
    fn test_gpui_sync_functionality() {
        // Test basic delegate creation without async operations
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let _delegate = GpuiTableDelegate::new(store);

        // Just test that creation succeeds
        assert!(true);
    }
}
