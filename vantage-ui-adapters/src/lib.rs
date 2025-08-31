use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;

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
#[derive(Debug)]
pub struct MockProductDataSet {
    products: Vec<TableRow>,
    columns: Vec<ColumnInfo>,
}

impl MockProductDataSet {
    pub fn new() -> Self {
        let columns = vec![
            ColumnInfo {
                name: "name".to_string(),
                data_type: "String".to_string(),
                sortable: true,
                editable: true,
            },
            ColumnInfo {
                name: "calories".to_string(),
                data_type: "Integer".to_string(),
                sortable: true,
                editable: true,
            },
            ColumnInfo {
                name: "price".to_string(),
                data_type: "Integer".to_string(),
                sortable: true,
                editable: true,
            },
            ColumnInfo {
                name: "inventory".to_string(),
                data_type: "Integer".to_string(),
                sortable: true,
                editable: true,
            },
        ];

        let products = vec![
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

        Self { products, columns }
    }
}

#[async_trait]
impl DataSet for MockProductDataSet {
    async fn row_count(&self) -> Result<usize> {
        Ok(self.products.len())
    }

    async fn column_info(&self) -> Result<Vec<ColumnInfo>> {
        Ok(self.columns.clone())
    }

    async fn fetch_rows(&self, start: usize, count: usize) -> Result<Vec<TableRow>> {
        let end = (start + count).min(self.products.len());
        if start >= self.products.len() {
            return Ok(vec![]);
        }
        Ok(self.products[start..end].to_vec())
    }

    async fn fetch_row(&self, index: usize) -> Result<TableRow> {
        self.products
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
