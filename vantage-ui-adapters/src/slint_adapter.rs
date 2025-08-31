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
    pub fn new(store: TableStore<D>) -> Self {
        let model = Self {
            store: Arc::new(store),
            rows: RefCell::new(VecModel::default()),
            column_names: RefCell::new(Vec::new()),
        };

        model.load_placeholder_data();
        model
    }

    fn load_placeholder_data(&self) {
        // Load column names
        if let Ok(mut column_names) = self.column_names.try_borrow_mut() {
            *column_names = vec![
                SharedString::from("Name"),
                SharedString::from("Calories"),
                SharedString::from("Price"),
                SharedString::from("Inventory"),
            ];
        }

        // Load placeholder data
        let placeholder_data = vec![
            vec!["Flux Capacitor Cupcake", "300", "120", "50"],
            vec!["DeLorean Doughnut", "250", "135", "30"],
            vec!["Time Traveler Tart", "200", "220", "20"],
            vec!["Enchantment Under the Sea Pie", "350", "299", "15"],
            vec!["Hoverboard Cookies", "150", "199", "40"],
        ];

        if let Ok(rows) = self.rows.try_borrow_mut() {
            let vec_data: Vec<SlintTableRow> = placeholder_data
                .into_iter()
                .map(|row| SlintTableRow {
                    cells: row.into_iter().map(SharedString::from).collect(),
                })
                .collect();
            rows.set_vec(vec_data);
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
            let new_row = SlintTableRow {
                cells: vec![
                    SharedString::from("New Product"),
                    SharedString::from("0"),
                    SharedString::from("0"),
                    SharedString::from("0"),
                ],
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
    pub fn new(store: TableStore<D>) -> Self {
        Self {
            model: SlintTableModel::new(store),
        }
    }

    pub fn model(&self) -> &SlintTableModel<D> {
        &self.model
    }

    pub fn as_model_rc(&self) -> ModelRc<SlintTableRow> {
        self.model.as_model_rc()
    }

    pub fn refresh(&self) {
        self.model.load_placeholder_data();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockProductDataSet;

    #[tokio::test]
    async fn test_slint_model_creation() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let model = SlintTableModel::new(store);

        // Test column names
        let column_names = model.column_names();
        assert_eq!(column_names.len(), 4);
        assert_eq!(column_names[0].as_str(), "Name");

        // Test row count
        assert_eq!(model.row_count(), 5);
    }

    #[tokio::test]
    async fn test_slint_row_data() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let model = SlintTableModel::new(store);

        // Test accessing cell data
        let first_cell = model.get_cell_value(0, 0);
        assert_eq!(first_cell, "Flux Capacitor Cupcake");

        let second_cell = model.get_cell_value(0, 1);
        assert_eq!(second_cell, "300");
    }

    #[tokio::test]
    async fn test_slint_table_wrapper() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = SlintTable::new(store);

        let model_rc = table.as_model_rc();
        assert_eq!(model_rc.row_count(), 5);

        // Test column names
        let columns = table.column_names();
        assert_eq!(columns.len(), 4);
        assert_eq!(columns[0].as_str(), "Name");
    }

    #[test]
    fn test_slint_cell_update() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = SlintTable::new(store);

        // Test cell update
        table.update_cell(0, 0, "Updated Product".to_string());

        // Verify local update
        let updated_value = table.get_cell_value(0, 0);
        assert_eq!(updated_value, "Updated Product");
    }

    #[test]
    fn test_slint_row_operations() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = SlintTable::new(store);

        let initial_count = table.row_count();
        assert_eq!(initial_count, 5);

        // Test adding row
        table.add_row();
        assert_eq!(table.row_count(), 6);

        // Test new row content
        let new_row_value = table.get_cell_value(5, 0);
        assert_eq!(new_row_value, "New Product");

        // Test removing row
        table.remove_row(5);
        assert_eq!(table.row_count(), 5);
    }

    #[tokio::test]
    async fn test_slint_refresh() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = SlintTable::new(store);

        // Test refresh doesn't panic
        table.refresh();
        assert_eq!(table.row_count(), 5);

        // Verify data is still correct after refresh
        let first_cell = table.get_cell_value(0, 0);
        assert_eq!(first_cell, "Flux Capacitor Cupcake");
    }
}
