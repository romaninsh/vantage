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
    pub async fn new(store: TableStore<D>) -> Self {
        let viewer = Self {
            store: Arc::new(store),
            cached_data: RefCell::new(None),
            cached_columns: RefCell::new(None),
        };

        viewer.load_data().await;
        viewer
    }

    async fn load_data(&self) {
        // Load column names
        if let Ok(column_info) = self.store.column_info().await {
            if let Ok(mut columns) = self.cached_columns.try_borrow_mut() {
                *columns = Some(column_info.into_iter().map(|col| col.name).collect());
            }
        }

        // Load row data
        if let Ok(row_count) = self.store.row_count().await {
            let _ = self.store.prefetch_range(0, row_count).await;

            if let Ok(mut data) = self.cached_data.try_borrow_mut() {
                let mut rows = Vec::new();
                for i in 0..row_count {
                    if let Ok(table_row) = self.store.get_row(i).await {
                        rows.push(table_row);
                    }
                }
                *data = Some(rows);
            }
        }
    }

    fn ensure_data_loaded(&self) {
        // Initialize empty data structures if not loaded
        if let Ok(mut columns) = self.cached_columns.try_borrow_mut() {
            if columns.is_none() {
                *columns = Some(vec![]);
            }
        }

        if let Ok(mut data) = self.cached_data.try_borrow_mut() {
            if data.is_none() {
                *data = Some(vec![]);
            }
        }
    }
}

impl<D: DataSet + 'static> RowViewer<EguiTableRow> for EguiTableViewer<D> {
    fn num_columns(&mut self) -> usize {
        self.ensure_data_loaded();
        if let Ok(columns) = self.cached_columns.try_borrow() {
            if let Some(ref cols) = *columns {
                return cols.len();
            }
        }
        0
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
        // Get column count from cached columns or use empty row
        let column_count = if let Ok(columns) = self.cached_columns.try_borrow() {
            if let Some(ref cols) = *columns {
                cols.len()
            } else {
                0
            }
        } else {
            0
        };

        EguiTableRow {
            index: 0,
            data: vec![CellValue::String(String::new()); column_count],
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
    pub async fn new(store: TableStore<D>) -> Self {
        let viewer = EguiTableViewer::new(store).await;
        let mut data_table = DataTable::new();

        // Load actual data from the viewer
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
        ui.add(egui_data_table::Renderer::new(
            &mut self.data_table,
            &mut self.viewer,
        ));

        ui.add_space(10.0);

        if ui.button("Refresh Data").clicked() {
            // Note: In a real app, you'd want to trigger async reload here
            // For now, just show current data
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
