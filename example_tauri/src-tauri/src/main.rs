use bakery_model3::{Client, connect_surrealdb, surrealdb};
use dataset_ui_adapters::{TableStore, VantageTableAdapter, tauri_adapter::TauriTable};
use vantage_table::any::AnyTable;

#[tokio::main]
async fn main() {
    connect_surrealdb()
        .await
        .expect("Failed to connect to SurrealDB");

    let client_table = AnyTable::from_table(Client::surreal_table(surrealdb()));
    let dataset = VantageTableAdapter::new(client_table).await;
    let store = TableStore::new(dataset);
    let table = TauriTable::new(store).await;

    tauri::Builder::default()
        .manage(table)
        .invoke_handler(tauri::generate_handler![get_table_data, get_table_columns])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
async fn get_table_data(
    table: tauri::State<'_, TauriTable<VantageTableAdapter>>,
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

    Ok(serde_json::json!({
        "rows": rows,
        "total": total_rows,
        "page": page,
        "page_size": page_size
    }))
}

#[tauri::command]
async fn get_table_columns(
    table: tauri::State<'_, TauriTable<VantageTableAdapter>>,
) -> Result<Vec<String>, String> {
    Ok(table.column_names())
}
