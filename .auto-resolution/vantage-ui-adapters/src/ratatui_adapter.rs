use crate::{DataSet, TableStore};
use ratatui::{
    layout::Constraint,
    style::{Color, Modifier, Style},
    widgets::{Cell, Row, Table, TableState},
};
use std::sync::Arc;

/// Ratatui adapter for displaying table data
pub struct RatatuiTableAdapter<D: DataSet> {
    store: Arc<TableStore<D>>,
    state: TableState,
    cached_rows: Vec<Vec<String>>,
    cached_headers: Vec<String>,
    row_count: usize,
    column_count: usize,
}

impl<D: DataSet + 'static> RatatuiTableAdapter<D> {
    pub fn new(store: TableStore<D>) -> Self {
        Self {
            store: Arc::new(store),
            state: TableState::default(),
            cached_rows: Vec::new(),
            cached_headers: Vec::new(),
            row_count: 0,
            column_count: 0,
        }
    }

    pub async fn refresh_data(&mut self) {
        // Get row count
        self.row_count = self.store.row_count().await.unwrap_or(0);

        // Get column headers
        self.cached_headers = match self.store.column_info().await {
            Ok(columns) => columns.into_iter().map(|col| col.name).collect(),
            Err(_) => vec!["Column 1".to_string(), "Column 2".to_string()],
        };

        self.column_count = self.cached_headers.len();

        // Get all rows (for simplicity - in production you'd want pagination)
        self.cached_rows = Vec::new();
        for i in 0..self.row_count {
            match self.store.get_row(i).await {
                Ok(row) => {
                    let string_row: Vec<String> =
                        row.into_iter().map(|cell| cell.as_string()).collect();
                    self.cached_rows.push(string_row);
                }
                Err(_) => {
                    self.cached_rows
                        .push(vec!["Error".to_string(); self.column_count]);
                }
            }
        }

        // Initialize selection
        if !self.cached_rows.is_empty() {
            self.state.select(Some(0));
        }
    }

    pub fn next_row(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.cached_rows.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous_row(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.cached_rows.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn state(&self) -> &TableState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut TableState {
        &mut self.state
    }

    pub fn create_table(&self) -> Table<'static> {
        let header_cells = self
            .cached_headers
            .iter()
            .map(|h| Cell::from(h.clone()))
            .collect::<Row>()
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );

        let rows: Vec<Row> = self
            .cached_rows
            .iter()
            .map(|item| {
                let cells = item.iter().map(|c| Cell::from(c.clone()));
                Row::new(cells).height(1)
            })
            .collect();

        // Calculate column constraints based on content
        let constraints: Vec<Constraint> = if self.cached_headers.is_empty() {
            vec![Constraint::Percentage(100)]
        } else {
            let col_count = self.cached_headers.len();
            vec![Constraint::Percentage((100 / col_count) as u16); col_count]
        };

        Table::new(rows, constraints)
            .header(header_cells)
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol(">> ")
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn column_count(&self) -> usize {
        self.column_count
    }

    pub fn selected_row(&self) -> Option<usize> {
        self.state.selected()
    }

    pub fn get_cell_value(&self, row: usize, col: usize) -> Option<String> {
        self.cached_rows.get(row)?.get(col).cloned()
    }
}
