use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;
use vantage_surrealdb::{SurrealDB, SurrealTableExt};
use vantage_table::{Entity, Table};

#[derive(Error, Debug)]
pub enum TableStoreError {
    #[error("Data fetch failed: {0}")]
    FetchError(String),
    #[error("Invalid row or column index")]
    IndexError,
    #[error("Cell value conversion failed")]
    ConversionError,
}

pub type Result<T> = std::result::Result<T, TableStoreError>;

/// Represents a cell value in the table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CellValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Null,
}

impl CellValue {
    pub fn as_string(&self) -> String {
        match self {
            CellValue::String(s) => s.clone(),
            CellValue::Integer(i) => i.to_string(),
            CellValue::Float(f) => f.to_string(),
            CellValue::Boolean(b) => b.to_string(),
            CellValue::Null => "".to_string(),
        }
    }
}

/// Column metadata
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub sortable: bool,
    pub editable: bool,
}

/// A row of data - just a vector of cell values
pub type TableRow = Vec<CellValue>;

/// Mock DataSet trait - represents your existing dry dataset
#[async_trait]
pub trait DataSet: Send + Sync {
    async fn row_count(&self) -> Result<usize>;
    async fn column_info(&self) -> Result<Vec<ColumnInfo>>;
    async fn fetch_rows(&self, start: usize, count: usize) -> Result<Vec<TableRow>>;
    async fn fetch_row(&self, index: usize) -> Result<TableRow>;

    // Optional mutation methods
    async fn update_cell(&self, _row: usize, _col: usize, _value: CellValue) -> Result<()> {
        Err(TableStoreError::FetchError(
            "Updates not supported".to_string(),
        ))
    }

    async fn insert_row(&self, _row: TableRow) -> Result<usize> {
        Err(TableStoreError::FetchError(
            "Inserts not supported".to_string(),
        ))
    }

    async fn delete_row(&self, _index: usize) -> Result<()> {
        Err(TableStoreError::FetchError(
            "Deletes not supported".to_string(),
        ))
    }
}

/// The intermediate caching layer - "TableStore" instead of "Hydrator"
#[derive(Debug)]
pub struct TableStore<D: DataSet> {
    dataset: Arc<D>,
    cached_rows: Arc<RwLock<HashMap<usize, TableRow>>>,
    cached_columns: Arc<RwLock<Option<Vec<ColumnInfo>>>>,
    cached_row_count: Arc<RwLock<Option<usize>>>,
    page_size: usize,
}

impl<D: DataSet> TableStore<D> {
    pub fn new(dataset: D) -> Self {
        Self {
            dataset: Arc::new(dataset),
            cached_rows: Arc::new(RwLock::new(HashMap::new())),
            cached_columns: Arc::new(RwLock::new(None)),
            cached_row_count: Arc::new(RwLock::new(None)),
            page_size: 100, // Default page size for efficient loading
        }
    }

    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size;
        self
    }

    pub async fn row_count(&self) -> Result<usize> {
        // Check cache first
        {
            let cached = self.cached_row_count.read().unwrap();
            if let Some(count) = *cached {
                return Ok(count);
            }
        }

        // Fetch and cache
        let count = self.dataset.row_count().await?;
        {
            let mut cached = self.cached_row_count.write().unwrap();
            *cached = Some(count);
        }
        Ok(count)
    }

    pub async fn column_info(&self) -> Result<Vec<ColumnInfo>> {
        // Check cache first
        {
            let cached = self.cached_columns.read().unwrap();
            if let Some(ref columns) = *cached {
                return Ok(columns.clone());
            }
        }

        // Fetch and cache
        let columns = self.dataset.column_info().await?;
        {
            let mut cached = self.cached_columns.write().unwrap();
            *cached = Some(columns.clone());
        }
        Ok(columns)
    }

    pub async fn cell_value(&self, row: usize, col: usize) -> Result<CellValue> {
        let table_row = self.get_row(row).await?;
        table_row
            .get(col)
            .cloned()
            .ok_or(TableStoreError::IndexError)
    }

    pub async fn get_row(&self, row_index: usize) -> Result<TableRow> {
        // Check cache first
        {
            let cached = self.cached_rows.read().unwrap();
            if let Some(row) = cached.get(&row_index) {
                return Ok(row.clone());
            }
        }

        // Fetch row and cache it
        let row = self.dataset.fetch_row(row_index).await?;
        {
            let mut cached = self.cached_rows.write().unwrap();
            cached.insert(row_index, row.clone());
        }
        Ok(row)
    }

    pub async fn prefetch_range(&self, start: usize, count: usize) -> Result<()> {
        let rows = self.dataset.fetch_rows(start, count).await?;
        {
            let mut cached = self.cached_rows.write().unwrap();
            for (i, row) in rows.into_iter().enumerate() {
                cached.insert(start + i, row);
            }
        }
        Ok(())
    }

    pub fn clear_cache(&self) {
        let mut rows = self.cached_rows.write().unwrap();
        let mut columns = self.cached_columns.write().unwrap();
        let mut count = self.cached_row_count.write().unwrap();

        rows.clear();
        *columns = None;
        *count = None;
    }

    // Mutation methods that invalidate cache as needed
    pub async fn update_cell(&self, row: usize, col: usize, value: CellValue) -> Result<()> {
        self.dataset.update_cell(row, col, value.clone()).await?;

        // Update cache if row is cached
        {
            let mut cached = self.cached_rows.write().unwrap();
            if let Some(cached_row) = cached.get_mut(&row) {
                if let Some(cell) = cached_row.get_mut(col) {
                    *cell = value;
                }
            }
        }
        Ok(())
    }

    pub async fn insert_row(&self, row: TableRow) -> Result<usize> {
        let new_index = self.dataset.insert_row(row).await?;

        // Invalidate count cache and shift cached rows if needed
        {
            let mut count = self.cached_row_count.write().unwrap();
            *count = None;
        }

        // For simplicity, clear all cached rows on insert
        // In production, you'd want smarter cache management
        {
            let mut cached = self.cached_rows.write().unwrap();
            cached.clear();
        }

        Ok(new_index)
    }

    pub async fn delete_row(&self, index: usize) -> Result<()> {
        self.dataset.delete_row(index).await?;

        // Invalidate count cache and clear cached rows
        {
            let mut count = self.cached_row_count.write().unwrap();
            *count = None;
        }

        {
            let mut cached = self.cached_rows.write().unwrap();
            cached.clear();
        }

        Ok(())
    }
}

// Mock implementation for testing

/// Adapter for vantage-table to DataSet interface
pub struct VantageTableAdapter<E: Entity> {
    _table: Table<SurrealDB, E>,
    cached_data: Vec<TableRow>,
    cached_columns: Vec<ColumnInfo>,
}

impl<E: Entity + Send + Sync + 'static> VantageTableAdapter<E> {
    pub async fn new(table: Table<SurrealDB, E>) -> Self {
        let column_names: Vec<String> = table.columns().keys().cloned().collect();
        let columns: Vec<ColumnInfo> = column_names
            .iter()
            .map(|name| ColumnInfo {
                name: name.clone(),
                data_type: "String".to_string(),
                sortable: true,
                editable: false,
            })
            .collect();

        let query = table.select_surreal();
        let data_source = table.data_source();
        let result = data_source.get(query).await;

        let rows = if let serde_json::Value::Array(array) = result {
            array
                .into_iter()
                .filter_map(|obj| {
                    if let serde_json::Value::Object(map) = obj {
                        let mut row = Vec::new();
                        for column_name in &column_names {
                            let value = map.get(column_name).unwrap_or(&serde_json::Value::Null);
                            let cell_value = match value {
                                serde_json::Value::String(s) => CellValue::String(s.clone()),
                                serde_json::Value::Number(n) => {
                                    if let Some(i) = n.as_i64() {
                                        CellValue::Integer(i)
                                    } else if let Some(f) = n.as_f64() {
                                        CellValue::Float(f)
                                    } else {
                                        CellValue::Null
                                    }
                                }
                                serde_json::Value::Bool(b) => CellValue::Boolean(*b),
                                serde_json::Value::Object(o) => {
                                    CellValue::String(serde_json::to_string(o).unwrap_or_default())
                                }
                                serde_json::Value::Array(a) => {
                                    CellValue::String(serde_json::to_string(a).unwrap_or_default())
                                }
                                _ => CellValue::Null,
                            };
                            row.push(cell_value);
                        }
                        if !row.is_empty() {
                            Some(row)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        Self {
            _table: table,
            cached_data: rows,
            cached_columns: columns,
        }
    }
}

#[async_trait]
impl<E: Entity + Send + Sync + 'static> DataSet for VantageTableAdapter<E> {
    async fn row_count(&self) -> Result<usize> {
        Ok(self.cached_data.len())
    }

    async fn column_info(&self) -> Result<Vec<ColumnInfo>> {
        Ok(self.cached_columns.clone())
    }

    async fn fetch_rows(&self, start: usize, count: usize) -> Result<Vec<TableRow>> {
        let end = (start + count).min(self.cached_data.len());
        if start >= self.cached_data.len() {
            return Ok(vec![]);
        }
        Ok(self.cached_data[start..end].to_vec())
    }

    async fn fetch_row(&self, index: usize) -> Result<TableRow> {
        self.cached_data
            .get(index)
            .cloned()
            .ok_or(TableStoreError::IndexError)
    }
}

// Framework-specific modules (behind feature flags)
#[cfg(feature = "egui")]
pub mod egui_adapter;

#[cfg(feature = "gpui")]
pub mod gpui_adapter;

#[cfg(feature = "slint")]
pub mod slint_adapter;

#[cfg(feature = "tauri")]
pub mod tauri_adapter;

#[cfg(feature = "ratatui")]
pub mod ratatui_adapter;

#[cfg(feature = "cursive")]
pub mod cursive_adapter;
