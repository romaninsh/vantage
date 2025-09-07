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
    rows: Arc<RwLock<Vec<TauriTableRow>>>,
    column_names: Arc<RwLock<Vec<String>>>,
    _phantom: std::marker::PhantomData<D>,
}

impl<D: DataSet + 'static> TauriTableModel<D> {
    pub fn new(_store: TableStore<D>) -> Self {
        let model = Self {
            rows: Arc::new(RwLock::new(Vec::new())),
            column_names: Arc::new(RwLock::new(Vec::new())),
            _phantom: std::marker::PhantomData,
        };

        model.load_placeholder_data();
        model
    }

    fn load_placeholder_data(&self) {
        if let Ok(mut column_names) = self.column_names.write() {
            *column_names = vec![
                "Name".to_string(),
                "Calories".to_string(),
                "Price".to_string(),
                "Inventory".to_string(),
            ];
        }

        let placeholder_data = vec![
            vec!["Flux Capacitor Cupcake", "300", "120", "50"],
            vec!["DeLorean Doughnut", "250", "135", "30"],
            vec!["Time Traveler Tart", "200", "220", "20"],
            vec!["Enchantment Under the Sea Pie", "350", "299", "15"],
            vec!["Hoverboard Cookies", "150", "199", "40"],
        ];

        if let Ok(mut rows) = self.rows.write() {
            *rows = placeholder_data
                .into_iter()
                .map(|row| TauriTableRow {
                    cells: row.into_iter().map(|s| s.to_string()).collect(),
                })
                .collect();
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
            rows.push(TauriTableRow {
                cells: vec![
                    "New Product".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                ],
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

    pub fn refresh(&self) {
        self.load_placeholder_data();
    }
}

/// Wrapper that creates the Tauri table with our adapter
pub struct TauriTable<D: DataSet> {
    model: TauriTableModel<D>,
}

impl<D: DataSet + 'static> TauriTable<D> {
    pub fn new(store: TableStore<D>) -> Self {
        Self {
            model: TauriTableModel::new(store),
        }
    }

    pub fn model(&self) -> &TauriTableModel<D> {
        &self.model
    }

    pub fn get_rows(&self) -> Vec<TauriTableRow> {
        self.model.get_rows()
    }

    pub fn refresh(&self) {
        self.model.refresh();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockProductDataSet;

    #[tokio::test]
    async fn test_tauri_table_creation() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = TauriTable::new(store);

        assert_eq!(table.row_count(), 5);
        assert_eq!(table.column_names().len(), 4);
        assert_eq!(table.column_names()[0].as_str(), "Name");
    }

    #[test]
    fn test_tauri_cell_operations() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = TauriTable::new(store);

        // Test getting cell value
        let cell_value = table.get_cell_value(0, 0);
        assert_eq!(cell_value, "Flux Capacitor Cupcake");

        // Test updating cell
        table.update_cell(0, 0, "Updated Product".to_string());
        let updated_value = table.get_cell_value(0, 0);
        assert_eq!(updated_value, "Updated Product");
    }

    #[test]
    fn test_tauri_row_operations() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = TauriTable::new(store);

        let initial_count = table.row_count();
        assert_eq!(initial_count, 5);

        // Test adding row
        table.add_row();
        assert_eq!(table.row_count(), 6);

        // Test removing row
        table.remove_row(5);
        assert_eq!(table.row_count(), 5);
    }
}
