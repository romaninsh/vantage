use dataset_ui_adapters::{slint_adapter::SlintTable, TableStore, VantageTableAdapter};
use bakery_model3::*;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::rc::Rc;
use tokio::runtime::Runtime;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    // Connect to SurrealDB and get client table
    let rt = Runtime::new().expect("Failed to create tokio runtime");

    let client_table = rt.block_on(async {
        bakery_model3::connect_surrealdb()
            .await
            .expect("Failed to connect to SurrealDB");
        Client::table()
    });

    let dataset = VantageTableAdapter::new(client_table);
    let store = TableStore::new(dataset);
    let table = SlintTable::new(store);
    let window = MainWindow::new()?;

    // Convert adapter data to Slint format
    let model_rc = table.as_model_rc();
    let mut slint_rows = Vec::new();

    for i in 0..model_rc.row_count() {
        if let Some(row) = model_rc.row_data(i) {
            let standard_items: Vec<slint::StandardListViewItem> = row
                .cells
                .iter()
                .map(|cell| slint::StandardListViewItem::from(cell.as_str()))
                .collect();
            let row_model = ModelRc::from(Rc::new(VecModel::from(standard_items)));
            slint_rows.push(row_model);
        }
    }

    let table_model = Rc::new(VecModel::from(slint_rows));
    window.set_table_rows(ModelRc::from(table_model));

    window.run()
}
