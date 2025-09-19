use crate::{DataSet, TableStore};
use slint::{Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::sync::Arc;

/// A row type for Slint - represents a single table row
#[derive(Clone, Debug)]
pub struct SlintTableRow {
    pub cells: Vec<SharedString>,
}

impl Default for SlintTableRow {
    fn default() -> Self {
        Self { cells: Vec::new() }
    }
}

/// Slint table model implementing the Model trait
pub struct SlintTableModel<D: DataSet> {
    store: Arc<TableStore<D>>,
    rows: RefCell<VecModel<SlintTableRow>>,
    column_names: RefCell<Vec<SharedString>>,
}

impl<D: DataSet + 'static> SlintTableModel<D> {
    pub async fn new(store: TableStore<D>) -> Self {
        let model = Self {
            store: Arc::new(store),
            rows: RefCell::new(VecModel::default()),
            column_names: RefCell::new(Vec::new()),
        };

        model.load_real_data().await;
        model
    }

    async fn load_real_data(&self) {
        // Load column names from the store
        if let Ok(column_info) = self.store.column_info().await {
            if let Ok(mut column_names) = self.column_names.try_borrow_mut() {
                *column_names = column_info
                    .into_iter()
                    .map(|col| SharedString::from(col.name))
                    .collect();
            }
        }

        // Load actual data from the store
        if let Ok(row_count) = self.store.row_count().await {
            // Prefetch all rows and then get them individually
            let _ = self.store.prefetch_range(0, row_count).await;

            if let Ok(rows) = self.rows.try_borrow_mut() {
                let mut vec_data = Vec::new();
                for i in 0..row_count {
                    if let Ok(table_row) = self.store.get_row(i).await {
                        let slint_row = SlintTableRow {
                            cells: table_row
                                .into_iter()
                                .map(|cell| SharedString::from(cell.as_string()))
                                .collect(),
                        };
                        vec_data.push(slint_row);
                    }
                }
                rows.set_vec(vec_data);
            }
        }
    }

    pub fn column_names(&self) -> Vec<SharedString> {
        self.column_names
            .try_borrow()
            .map(|names| names.clone())
            .unwrap_or_default()
    }

    pub fn update_cell(&self, row: usize, col: usize, value: String) {
        if let Ok(rows) = self.rows.try_borrow_mut() {
            if let Some(mut row_data) = rows.row_data(row) {
                if col < row_data.cells.len() {
                    row_data.cells[col] = SharedString::from(value);
                    rows.set_row_data(row, row_data);
                }
            }
        }
    }

    pub fn add_row(&self) {
        if let Ok(rows) = self.rows.try_borrow_mut() {
            // Get column count from existing data or use empty row
            let column_count = if let Some(first_row) = rows.row_data(0) {
                first_row.cells.len()
            } else {
                0
            };

            let new_row = SlintTableRow {
                cells: vec![SharedString::from(""); column_count],
            };
            rows.push(new_row);
        }
    }

    pub fn remove_row(&self, index: usize) {
        if let Ok(rows) = self.rows.try_borrow_mut() {
            if index < rows.row_count() {
                rows.remove(index);
            }
        }
    }

    pub fn as_model_rc(&self) -> ModelRc<SlintTableRow> {
        if let Ok(rows) = self.rows.try_borrow() {
            let mut vec_data = Vec::new();
            for i in 0..rows.row_count() {
                if let Some(row) = rows.row_data(i) {
                    vec_data.push(row);
                }
            }
            ModelRc::new(VecModel::from(vec_data))
        } else {
            ModelRc::new(VecModel::default())
        }
    }

    pub fn row_count(&self) -> usize {
        self.rows
            .try_borrow()
            .map(|rows| rows.row_count())
            .unwrap_or(0)
    }

    pub fn get_cell_value(&self, row: usize, col: usize) -> String {
        if let Ok(rows) = self.rows.try_borrow() {
            if let Some(row_data) = rows.row_data(row) {
                if let Some(cell) = row_data.cells.get(col) {
                    return cell.to_string();
                }
            }
        }
        String::new()
    }
}

/// Wrapper struct for easier usage in Slint applications
pub struct SlintTable<D: DataSet> {
    model: SlintTableModel<D>,
}

impl<D: DataSet + 'static> SlintTable<D> {
    pub async fn new(store: TableStore<D>) -> Self {
        Self {
            model: SlintTableModel::new(store).await,
        }
    }

    pub fn model(&self) -> &SlintTableModel<D> {
        &self.model
    }

    pub fn as_model_rc(&self) -> ModelRc<SlintTableRow> {
        self.model.as_model_rc()
    }

    pub async fn refresh(&self) {
        self.model.load_real_data().await;
    }

    pub fn column_names(&self) -> Vec<SharedString> {
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

impl<D: DataSet + 'static> std::fmt::Debug for SlintTable<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlintTable")
            .field("row_count", &self.row_count())
            .field("column_count", &self.column_names().len())
            .finish()
    }
}
