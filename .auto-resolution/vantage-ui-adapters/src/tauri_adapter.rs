use crate::{DataSet, TableStore};
use std::sync::{Arc, RwLock};

/// A row type for Tauri - represents a single table row
#[derive(Clone, Debug)]
pub struct TauriTableRow {
    pub cells: Vec<String>,
}

impl Default for TauriTableRow {
    fn default() -> Self {
        Self { cells: Vec::new() }
    }
}

/// Tauri table model implementing similar pattern to other adapters
pub struct TauriTableModel<D: DataSet> {
    store: Arc<TableStore<D>>,
    rows: Arc<RwLock<Vec<TauriTableRow>>>,
    column_names: Arc<RwLock<Vec<String>>>,
}

impl<D: DataSet + 'static> TauriTableModel<D> {
    pub async fn new(store: TableStore<D>) -> Self {
        let model = Self {
            store: Arc::new(store),
            rows: Arc::new(RwLock::new(Vec::new())),
            column_names: Arc::new(RwLock::new(Vec::new())),
        };

        model.load_data().await;
        model
    }

    async fn load_data(&self) {
        // Load column names
        if let Ok(column_info) = self.store.column_info().await {
            if let Ok(mut column_names) = self.column_names.write() {
                *column_names = column_info.into_iter().map(|col| col.name).collect();
            }
        }

        // Load row data
        if let Ok(row_count) = self.store.row_count().await {
            let _ = self.store.prefetch_range(0, row_count).await;

            if let Ok(mut rows) = self.rows.write() {
                let mut row_data = Vec::new();
                for i in 0..row_count {
                    if let Ok(table_row) = self.store.get_row(i).await {
                        let tauri_row = TauriTableRow {
                            cells: table_row.into_iter().map(|cell| cell.as_string()).collect(),
                        };
                        row_data.push(tauri_row);
                    }
                }
                *rows = row_data;
            }
        }
    }

    pub fn column_names(&self) -> Vec<String> {
        self.column_names
            .read()
            .map(|names| names.clone())
            .unwrap_or_default()
    }

    pub fn update_cell(&self, row: usize, col: usize, value: String) {
        if let Ok(mut rows) = self.rows.write() {
            if let Some(row_data) = rows.get_mut(row) {
                if col < row_data.cells.len() {
                    row_data.cells[col] = value;
                }
            }
        }
    }

    pub fn add_row(&self) {
        if let Ok(mut rows) = self.rows.write() {
            // Get column count from existing data or use empty row
            let column_count = if let Some(first_row) = rows.first() {
                first_row.cells.len()
            } else {
                0
            };

            rows.push(TauriTableRow {
                cells: vec![String::new(); column_count],
            });
        }
    }

    pub fn remove_row(&self, index: usize) {
        if let Ok(mut rows) = self.rows.write() {
            if index < rows.len() {
                rows.remove(index);
            }
        }
    }

    pub fn get_rows(&self) -> Vec<TauriTableRow> {
        self.rows
            .read()
            .map(|rows| rows.clone())
            .unwrap_or_default()
    }

    pub fn row_count(&self) -> usize {
        self.rows.read().map(|rows| rows.len()).unwrap_or(0)
    }

    pub fn get_cell_value(&self, row: usize, col: usize) -> String {
        if let Ok(rows) = self.rows.read() {
            if let Some(row_data) = rows.get(row) {
                if let Some(cell) = row_data.cells.get(col) {
                    return cell.clone();
                }
            }
        }
        String::new()
    }

    pub async fn refresh(&self) {
        self.load_data().await;
    }
}

/// Wrapper that creates the Tauri table with our adapter
pub struct TauriTable<D: DataSet> {
    model: TauriTableModel<D>,
}

impl<D: DataSet + 'static> TauriTable<D> {
    pub async fn new(store: TableStore<D>) -> Self {
        Self {
            model: TauriTableModel::new(store).await,
        }
    }

    pub fn model(&self) -> &TauriTableModel<D> {
        &self.model
    }

    pub fn get_rows(&self) -> Vec<TauriTableRow> {
        self.model.get_rows()
    }

    pub async fn refresh(&self) {
        self.model.refresh().await;
    }

    pub fn column_names(&self) -> Vec<String> {
        self.model.column_names()
    }

    pub fn update_cell(&self, row: usize, col: usize, value: String) {
        self.model.update_cell(row, col, value);
    }

    pub fn add_row(&self) {
        self.model.add_row();
    }

    pub fn remove_row(&self, index: usize) {
        self.model.remove_row(index);
    }

    pub fn row_count(&self) -> usize {
        self.model.row_count()
    }

    pub fn get_cell_value(&self, row: usize, col: usize) -> String {
        self.model.get_cell_value(row, col)
    }
}

impl<D: DataSet + 'static> std::fmt::Debug for TauriTable<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TauriTable")
            .field("row_count", &self.row_count())
            .field("column_count", &self.column_names().len())
            .finish()
    }
}
