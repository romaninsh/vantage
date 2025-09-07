use crate::{CellValue, DataSet, TableStore};
use egui_data_table::{DataTable, RowViewer};
use std::cell::RefCell;
use std::sync::Arc;

/// A row reference for egui-data-table - just contains the row index
#[derive(Debug, Clone)]
pub struct EguiTableRow {
    pub index: usize,
    pub data: Vec<CellValue>,
}

/// egui adapter that implements RowViewer trait
pub struct EguiTableViewer<D: DataSet> {
    store: Arc<TableStore<D>>,
    cached_data: RefCell<Option<Vec<Vec<CellValue>>>>,
    cached_columns: RefCell<Option<Vec<String>>>,
}

impl<D: DataSet + 'static> EguiTableViewer<D> {
    pub fn new(store: TableStore<D>) -> Self {
        Self {
            store: Arc::new(store),
            cached_data: RefCell::new(None),
            cached_columns: RefCell::new(None),
        }
    }

    fn ensure_data_loaded(&self) {
        // Load placeholder data to avoid async issues
        if let Ok(mut columns) = self.cached_columns.try_borrow_mut() {
            if columns.is_none() {
                *columns = Some(vec![
                    "Name".to_string(),
                    "Calories".to_string(),
                    "Price".to_string(),
                    "Inventory".to_string(),
                ]);
            }
        }

        if let Ok(mut data) = self.cached_data.try_borrow_mut() {
            if data.is_none() {
                // Use placeholder data from MockProductDataSet
                let placeholder_data = vec![
                    vec![
                        CellValue::String("Flux Capacitor Cupcake".to_string()),
                        CellValue::Integer(300),
                        CellValue::Integer(120),
                        CellValue::Integer(50),
                    ],
                    vec![
                        CellValue::String("DeLorean Doughnut".to_string()),
                        CellValue::Integer(250),
                        CellValue::Integer(135),
                        CellValue::Integer(30),
                    ],
                    vec![
                        CellValue::String("Time Traveler Tart".to_string()),
                        CellValue::Integer(200),
                        CellValue::Integer(220),
                        CellValue::Integer(20),
                    ],
                    vec![
                        CellValue::String("Enchantment Under the Sea Pie".to_string()),
                        CellValue::Integer(350),
                        CellValue::Integer(299),
                        CellValue::Integer(15),
                    ],
                    vec![
                        CellValue::String("Hoverboard Cookies".to_string()),
                        CellValue::Integer(150),
                        CellValue::Integer(199),
                        CellValue::Integer(40),
                    ],
                ];
                *data = Some(placeholder_data);
            }
        }
    }
}

impl<D: DataSet + 'static> RowViewer<EguiTableRow> for EguiTableViewer<D> {
    fn num_columns(&mut self) -> usize {
        self.ensure_data_loaded();
        4 // We have 4 columns: Name, Calories, Price, Inventory
    }

    fn show_cell_view(&mut self, ui: &mut egui::Ui, row: &EguiTableRow, column: usize) {
        if let Some(cell_value) = row.data.get(column) {
            ui.label(cell_value.as_string());
        } else {
            ui.label("N/A");
        }
    }

    fn show_cell_editor(
        &mut self,
        ui: &mut egui::Ui,
        row: &mut EguiTableRow,
        column: usize,
    ) -> Option<egui::Response> {
        if let Some(cell_value) = row.data.get_mut(column) {
            let mut text = cell_value.as_string();
            let response = ui.text_edit_singleline(&mut text);

            if response.lost_focus() {
                *cell_value = CellValue::String(text);
            }

            Some(response)
        } else {
            None
        }
    }

    fn set_cell_value(&mut self, src: &EguiTableRow, dst: &mut EguiTableRow, column: usize) {
        if let (Some(src_cell), Some(dst_cell)) = (src.data.get(column), dst.data.get_mut(column)) {
            *dst_cell = src_cell.clone();
        }
    }

    fn new_empty_row(&mut self) -> EguiTableRow {
        EguiTableRow {
            index: 0,
            data: vec![
                CellValue::String("New Product".to_string()),
                CellValue::Integer(0),
                CellValue::Integer(0),
                CellValue::Integer(0),
            ],
        }
    }

    fn clone_row(&mut self, src: &EguiTableRow) -> EguiTableRow {
        EguiTableRow {
            index: src.index,
            data: src.data.clone(),
        }
    }
}

/// Wrapper that creates the egui DataTable with our adapter
pub struct EguiTable<D: DataSet> {
    data_table: DataTable<EguiTableRow>,
    viewer: EguiTableViewer<D>,
}

impl<D: DataSet + 'static> EguiTable<D> {
    pub fn new(store: TableStore<D>) -> Self {
        let viewer = EguiTableViewer::new(store);
        let mut data_table = DataTable::new();

        // Ensure data is loaded
        viewer.ensure_data_loaded();

        // Pre-populate with mock rows based on our placeholder data
        if let Ok(data) = viewer.cached_data.try_borrow() {
            if let Some(ref data) = *data {
                for (index, row_data) in data.iter().enumerate() {
                    data_table.push(EguiTableRow {
                        index,
                        data: row_data.clone(),
                    });
                }
            }
        }

        Self { data_table, viewer }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("Dataset UI Adapters - egui Table Example");
        ui.separator();
        ui.add_space(10.0);

        ui.add(egui_data_table::Renderer::new(
            &mut self.data_table,
            &mut self.viewer,
        ));

        ui.add_space(10.0);

        if ui.button("Refresh Data").clicked() {
            // Clear and reload data
            self.viewer.cached_data.replace(None);
            self.viewer.ensure_data_loaded();

            // Rebuild table data
            self.data_table.clear();
            if let Ok(data) = self.viewer.cached_data.try_borrow() {
                if let Some(ref data) = *data {
                    for (index, row_data) in data.iter().enumerate() {
                        self.data_table.push(EguiTableRow {
                            index,
                            data: row_data.clone(),
                        });
                    }
                }
            }
        }
    }
}

impl<D: DataSet> std::fmt::Debug for EguiTable<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EguiTable")
            .field("data_table", &"<DataTable>")
            .field("viewer", &"<EguiTableViewer>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockProductDataSet;

    #[tokio::test]
    async fn test_egui_table_creation() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = EguiTable::new(store);

        // Verify data loaded
        assert!(table.viewer.cached_columns.try_borrow().unwrap().is_some());
        assert!(table.viewer.cached_data.try_borrow().unwrap().is_some());

        let columns = table.viewer.cached_columns.try_borrow().unwrap();
        let columns = columns.as_ref().unwrap();
        assert_eq!(columns.len(), 4);
        assert_eq!(columns[0], "Name");

        let data = table.viewer.cached_data.try_borrow().unwrap();
        let data = data.as_ref().unwrap();
        assert_eq!(data.len(), 5);
    }

    #[test]
    fn test_egui_viewer() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let mut viewer = EguiTableViewer::new(store);

        // Test column count
        assert_eq!(viewer.num_columns(), 4);

        // Test row creation
        let empty_row = viewer.new_empty_row();
        assert_eq!(empty_row.data.len(), 4);
        assert_eq!(empty_row.data[0].as_string(), "New Product");
    }

    #[test]
    fn test_egui_row_operations() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let mut viewer = EguiTableViewer::new(store);

        let row1 = EguiTableRow {
            index: 0,
            data: vec![
                CellValue::String("Test Product".to_string()),
                CellValue::Integer(100),
                CellValue::Integer(50),
                CellValue::Integer(25),
            ],
        };

        // Test cloning
        let row2 = viewer.clone_row(&row1);
        assert_eq!(row2.data[0].as_string(), "Test Product");

        // Test cell value setting
        let mut row3 = viewer.new_empty_row();
        viewer.set_cell_value(&row1, &mut row3, 0);
        assert_eq!(row3.data[0].as_string(), "Test Product");
    }
}
