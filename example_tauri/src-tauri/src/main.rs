use dataset_ui_adapters::{tauri_adapter::TauriTable, MockProductDataSet, TableStore};

#[tokio::main]
async fn main() {
    let dataset = MockProductDataSet::new();
    let store = TableStore::new(dataset);
    let table = TauriTable::new(store);

    tauri::Builder::default()
        .manage(table)
        .invoke_handler(tauri::generate_handler![
            get_table_data,
            get_table_columns
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
async fn get_table_data(
    table: tauri::State<'_, TauriTable<MockProductDataSet>>,
    page: Option<usize>,
    page_size: Option<usize>,
) -> Result<serde_json::Value, String> {
    let page = page.unwrap_or(0);
    let page_size = page_size.unwrap_or(100);

    let all_rows = table.get_rows();
    let total_rows = all_rows.len();
    let start = page * page_size;
    let end = (start + page_size).min(total_rows);

    let mut rows = Vec::new();
    for i in start..end {
        if let Some(row) = all_rows.get(i) {
            rows.push(row.cells.clone());
        }
    }

    let result = serde_json::json!({
        "rows": rows,
        "total": total_rows,
        "page": page,
        "page_size": page_size
    });

    Ok(result)
}

#[tauri::command]
async fn get_table_columns(
    table: tauri::State<'_, TauriTable<MockProductDataSet>>,
) -> Result<Vec<String>, String> {
    let columns = table.column_names();
    Ok(columns)
}
