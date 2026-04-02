use crate::{DataSet, TableStore};
use cursive::{
    event::Key,
    traits::{Nameable, Resizable},
    views::{Dialog, LinearLayout, TextView},
    Cursive, CursiveExt, View,
};
use cursive_table_view::{TableView, TableViewItem};
use std::cmp::Ordering;
use std::sync::Arc;

/// A row item for the Cursive table view
#[derive(Clone, Debug)]
pub struct TableRow {
    pub data: Vec<String>,
    pub index: usize,
}

impl TableViewItem<usize> for TableRow {
    fn to_column(&self, column: usize) -> String {
        self.data.get(column).cloned().unwrap_or_default()
    }

    fn cmp(&self, other: &Self, column: usize) -> Ordering {
        let self_val = self.to_column(column);
        let other_val = other.to_column(column);

        // Try to parse as numbers first for better sorting
        if let (Ok(a), Ok(b)) = (self_val.parse::<f64>(), other_val.parse::<f64>()) {
            a.partial_cmp(&b).unwrap_or(Ordering::Equal)
        } else {
            self_val.cmp(&other_val)
        }
    }
}

/// Cursive adapter for displaying table data
pub struct CursiveTableAdapter<D: DataSet> {
    store: Arc<TableStore<D>>,
    table_view: TableView<TableRow, usize>,
    cached_data: Vec<TableRow>,
    column_headers: Vec<String>,
}

impl<D: DataSet + 'static> CursiveTableAdapter<D> {
    pub async fn new(store: TableStore<D>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut adapter = Self {
            store: Arc::new(store),
            table_view: TableView::<TableRow, usize>::new(),
            cached_data: Vec::new(),
            column_headers: Vec::new(),
        };

        adapter.refresh_data().await?;
        Ok(adapter)
    }

    pub async fn refresh_data(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let store = self.store.clone();

        // Get column info
        self.column_headers = match store.column_info().await {
            Ok(columns) => columns.into_iter().map(|col| col.name).collect(),
            Err(_) => vec!["Column 1".to_string(), "Column 2".to_string()],
        };

        // Set up table columns
        self.table_view.clear();
        for (idx, header) in self.column_headers.iter().enumerate() {
            self.table_view.add_column(idx, header, |c| {
                c.ordering(Ordering::Greater)
                    .align(cursive::align::HAlign::Left)
            });
        }

        // Get row count and load data
        let store = self.store.clone();
        let row_count = store.row_count().await.unwrap_or(0);

        // Load all rows
        self.cached_data.clear();
        let store = self.store.clone();

        for i in 0..row_count {
            let row_data = match store.get_row(i).await {
                Ok(row) => row
                    .into_iter()
                    .map(|cell| cell.as_string())
                    .collect::<Vec<_>>(),
                Err(_) => vec!["Error".to_string(); self.column_headers.len()],
            };

            self.cached_data.push(TableRow {
                data: row_data,
                index: i,
            });
        }

        // Set the data in the table view
        self.table_view.set_items(self.cached_data.clone());

        Ok(())
    }

    pub fn into_view(self) -> impl View {
        LinearLayout::vertical()
            .child(TextView::new("Bakery Model 3 - Client List").center())
            .child(TextView::new("Use ↑/↓ to navigate, Enter to select, q to quit").center())
            .child(self.table_view.with_name("table").min_size((80, 20)))
    }

    pub fn get_table_view(&self) -> &TableView<TableRow, usize> {
        &self.table_view
    }

    pub fn get_selected_row(&self) -> Option<&TableRow> {
        let selected_index = self.table_view.item()?;
        self.cached_data.get(selected_index)
    }

    pub fn row_count(&self) -> usize {
        self.cached_data.len()
    }

    pub fn column_count(&self) -> usize {
        self.column_headers.len()
    }

    pub fn get_column_headers(&self) -> &[String] {
        &self.column_headers
    }

    pub fn get_cached_data(&self) -> &[TableRow] {
        &self.cached_data
    }
}

/// Wrapper for creating a complete Cursive application with table
pub struct CursiveTableApp<D: DataSet + 'static> {
    adapter: CursiveTableAdapter<D>,
}

impl<D: DataSet + 'static> CursiveTableApp<D> {
    pub async fn new(store: TableStore<D>) -> Result<Self, Box<dyn std::error::Error>> {
        let adapter = CursiveTableAdapter::new(store).await?;

        Ok(Self { adapter })
    }

    pub fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let mut siv = Cursive::default();

        // Add global callbacks
        siv.add_global_callback('q', |s| s.quit());
        siv.add_global_callback(Key::Esc, |s| s.quit());

        // Get data before consuming the adapter
        let column_headers = self.adapter.get_column_headers().to_vec();
        let cached_data = self.adapter.get_cached_data().to_vec();

        // Add the table view
        let view = self.adapter.into_view();
        siv.add_layer(
            Dialog::around(view)
                .title("Bakery Model 3 - SurrealDB Clients")
                .button("Refresh", |s| {
                    // Note: In a real app, you'd want to handle refresh properly
                    s.add_layer(Dialog::info("Refresh functionality would go here"));
                })
                .button("Quit", |s| s.quit()),
        );

        // Set up callbacks for the table - Enter key to show details
        siv.call_on_name("table", move |table: &mut TableView<TableRow, usize>| {
            let headers = column_headers.clone();
            let data = cached_data.clone();
            table.set_on_submit(move |siv, _row, index| {
                if let Some(row_data) = data.get(index) {
                    let mut details = String::from("Row Details:\n\n");
                    for (i, header) in headers.iter().enumerate() {
                        details.push_str(&format!("{}: {}\n", header, row_data.to_column(i)));
                    }
                    siv.add_layer(Dialog::text(details).title("Row Information").button(
                        "Close",
                        |s| {
                            s.pop_layer();
                        },
                    ));
                }
            });
        });

        siv.run();
        Ok(())
    }
}
