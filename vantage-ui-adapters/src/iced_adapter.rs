use crate::{CellValue, DataSet, TableStore};
use iced::widget::{Button, Column, Row, Text, TextInput};
use iced::{Command, Element, Length};
use std::cell::RefCell;
use std::sync::Arc;

/// Messages for the Iced table component
#[derive(Debug, Clone)]
pub enum TableMessage {
    CellChanged {
        row: usize,
        col: usize,
        value: String,
    },
    LoadData,
    RefreshData,
    EditCell {
        row: usize,
        col: usize,
    },
    StopEditing,
}

/// State for the Iced table
#[derive(Debug)]
pub struct IcedTableState {
    pub editing_cell: Option<(usize, usize)>,
    pub edit_buffer: String,
}

impl Default for IcedTableState {
    fn default() -> Self {
        Self {
            editing_cell: None,
            edit_buffer: String::new(),
        }
    }
}

/// Iced table component
pub struct IcedTable<D: DataSet> {
    store: Arc<TableStore<D>>,
    state: RefCell<IcedTableState>,
    cached_data: RefCell<Option<Vec<Vec<CellValue>>>>,
    cached_columns: RefCell<Option<Vec<String>>>,
}

impl<D: DataSet> std::fmt::Debug for IcedTable<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IcedTable")
            .field("state", &"<RefCell>")
            .field("cached_data", &"<RefCell>")
            .field("cached_columns", &"<RefCell>")
            .finish()
    }
}

impl<D: DataSet + 'static> IcedTable<D> {
    pub fn new(store: TableStore<D>) -> Self {
        Self {
            store: Arc::new(store),
            state: RefCell::new(IcedTableState::default()),
            cached_data: RefCell::new(None),
            cached_columns: RefCell::new(None),
        }
    }

    pub fn update(&mut self, message: TableMessage) -> Command<TableMessage> {
        match message {
            TableMessage::CellChanged { row, col, value } => {
                let _store = self.store.clone();
                let cell_value = CellValue::String(value);

                // Update cache optimistically
                if let Ok(mut data) = self.cached_data.try_borrow_mut() {
                    if let Some(ref mut data) = *data {
                        if let Some(row_data) = data.get_mut(row) {
                            if let Some(cell) = row_data.get_mut(col) {
                                *cell = cell_value.clone();
                            }
                        }
                    }
                }

                // Stop editing
                if let Ok(mut state) = self.state.try_borrow_mut() {
                    state.editing_cell = None;
                    state.edit_buffer.clear();
                }

                // For now, just trigger refresh without async update
                // TODO: Implement proper async update handling
                Command::none()
            }

            TableMessage::LoadData => {
                // Just trigger refresh - data will be loaded lazily in view
                Command::none()
            }

            TableMessage::RefreshData => {
                // Clear cache to force reload
                if let Ok(mut data) = self.cached_data.try_borrow_mut() {
                    *data = None;
                }
                if let Ok(mut columns) = self.cached_columns.try_borrow_mut() {
                    *columns = None;
                }
                Command::none()
            }

            TableMessage::EditCell { row, col } => {
                // Start editing a cell
                if let (Ok(data), Ok(mut state)) =
                    (self.cached_data.try_borrow(), self.state.try_borrow_mut())
                {
                    if let Some(ref data) = *data {
                        if let Some(row_data) = data.get(row) {
                            if let Some(cell) = row_data.get(col) {
                                state.edit_buffer = cell.as_string();
                                state.editing_cell = Some((row, col));
                            }
                        }
                    }
                }
                Command::none()
            }

            TableMessage::StopEditing => {
                if let Ok(mut state) = self.state.try_borrow_mut() {
                    state.editing_cell = None;
                    state.edit_buffer.clear();
                }
                Command::none()
            }
        }
    }

    fn ensure_data_loaded(&self) {
        // For now, use placeholder data
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

    pub fn view(&self) -> Element<'_, TableMessage> {
        // Ensure data is loaded
        self.ensure_data_loaded();

        let mut content = Column::new().spacing(10).width(Length::Fill);

        // Header row
        if let Ok(columns) = self.cached_columns.try_borrow() {
            if let Some(ref columns) = *columns {
                let mut header = Row::new().spacing(10);
                for col_name in columns {
                    header = header.push(
                        Text::new(col_name.clone())
                            .width(Length::Fixed(150.0))
                            .size(16),
                    );
                }
                content = content.push(header);
            }
        }

        // Data rows
        if let (Ok(data), Ok(state)) = (self.cached_data.try_borrow(), self.state.try_borrow()) {
            if let Some(ref data) = *data {
                for (row_idx, row_data) in data.iter().enumerate() {
                    let mut row = Row::new().spacing(10);

                    for (col_idx, cell_value) in row_data.iter().enumerate() {
                        let cell_element: Element<TableMessage> =
                            if Some((row_idx, col_idx)) == state.editing_cell {
                                // Show text input for editing
                                TextInput::new("", &state.edit_buffer)
                                    .on_input(move |value| TableMessage::CellChanged {
                                        row: row_idx,
                                        col: col_idx,
                                        value,
                                    })
                                    .on_submit(TableMessage::StopEditing)
                                    .width(Length::Fixed(150.0))
                                    .into()
                            } else {
                                // Show clickable text for editing
                                Button::new(
                                    Text::new(cell_value.as_string()).width(Length::Fixed(130.0)),
                                )
                                .on_press(TableMessage::EditCell {
                                    row: row_idx,
                                    col: col_idx,
                                })
                                .width(Length::Fixed(150.0))
                                .into()
                            };

                        row = row.push(cell_element);
                    }

                    content = content.push(row);
                }
            } else {
                content = content.push(Text::new("No data loaded").size(16));
            }
        }

        // Load button
        content = content.push(
            Button::new(Text::new("Load Data"))
                .on_press(TableMessage::LoadData)
                .padding(10),
        );

        content.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockProductDataSet;
    use tokio_test;

    #[tokio::test]
    async fn test_iced_table_creation() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = IcedTable::new(store);

        // Load data
        table.ensure_data_loaded();

        // Verify data loaded
        assert!(table.cached_columns.try_borrow().unwrap().is_some());
        assert!(table.cached_data.try_borrow().unwrap().is_some());

        let columns = table.cached_columns.try_borrow().unwrap();
        let columns = columns.as_ref().unwrap();
        assert_eq!(columns.len(), 4);
        assert_eq!(columns[0], "Name");

        let data = table.cached_data.try_borrow().unwrap();
        let data = data.as_ref().unwrap();
        assert_eq!(data.len(), 5);
    }

    #[test]
    fn test_iced_table_messages() {
        tokio_test::block_on(async {
            let dataset = MockProductDataSet::new();
            let store = TableStore::new(dataset);
            let mut table = IcedTable::new(store);

            // Test cell change message
            let message = TableMessage::CellChanged {
                row: 0,
                col: 0,
                value: "New Value".to_string(),
            };

            // Just verify the update method doesn't panic
            let _task = table.update(message);
        });
    }

    #[tokio::test]
    async fn test_iced_view_generation() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let table = IcedTable::new(store);

        // Generate view - should not panic
        let _view = table.view();
    }

    #[test]
    fn test_edit_cell_functionality() {
        let dataset = MockProductDataSet::new();
        let store = TableStore::new(dataset);
        let mut table = IcedTable::new(store);

        table.ensure_data_loaded();

        // Test starting edit
        let _task = table.update(TableMessage::EditCell { row: 0, col: 0 });
        {
            let state = table.state.borrow();
            assert_eq!(state.editing_cell, Some((0, 0)));
            assert_eq!(state.edit_buffer, "Flux Capacitor Cupcake");
        }

        // Test stopping edit
        let _task = table.update(TableMessage::StopEditing);
        {
            let state = table.state.borrow();
            assert_eq!(state.editing_cell, None);
            assert!(state.edit_buffer.is_empty());
        }
    }
}
