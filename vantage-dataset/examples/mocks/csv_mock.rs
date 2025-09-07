// examples/mocks/csv_mock.rs

use csv::ReaderBuilder;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use vantage_dataset::dataset::{DataSetError, ReadableDataSet, Result};

/// MockCsv contains hardcoded CSV data as strings
#[derive(Debug, Clone)]
pub struct MockCsv {
    files: HashMap<String, String>,
}

impl MockCsv {
    pub fn new() -> Self {
        let mut files = HashMap::new();

        // Add users.csv data
        files.insert(
            "users.csv".to_string(),
            "id,name,email,age\n1,Alice,alice@example.com,25\n2,Bob,bob@example.com,30\n3,Charlie,charlie@example.com,35".to_string(),
        );

        // Add products.csv data
        files.insert(
            "products.csv".to_string(),
            "id,name,price,category\n1,Laptop,999.99,Electronics\n2,Chair,149.99,Furniture\n3,Book,19.99,Education".to_string(),
        );

        Self { files }
    }

    pub fn get_file_content(&self, filename: &str) -> Option<&String> {
        self.files.get(filename)
    }
}

/// CsvFile represents a typed CSV file dataset
pub struct CsvFile<T> {
    csv_ds: MockCsv,
    filename: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> CsvFile<T>
where
    T: DeserializeOwned,
{
    pub fn new(csv_ds: &MockCsv, filename: &str) -> Self {
        Self {
            csv_ds: csv_ds.clone(),
            filename: filename.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<T> ReadableDataSet<T> for CsvFile<T>
where
    T: DeserializeOwned + Send + Sync,
{
    async fn get(&self) -> Result<Vec<T>> {
        self.get_as().await
    }

    async fn get_some(&self) -> Result<Option<T>> {
        self.get_some_as().await
    }

    async fn get_as<U>(&self) -> Result<Vec<U>>
    where
        U: DeserializeOwned,
    {
        let content = self
            .csv_ds
            .get_file_content(&self.filename)
            .ok_or_else(|| DataSetError::other(format!("File '{}' not found", self.filename)))?;

        let mut reader = ReaderBuilder::new()
            .has_headers(true)
            .from_reader(content.as_bytes());

        let mut records = Vec::new();
        for result in reader.deserialize() {
            let record: U = result
                .map_err(|e| DataSetError::other(format!("CSV deserialization error: {}", e)))?;
            records.push(record);
        }

        Ok(records)
    }

    async fn get_some_as<U>(&self) -> Result<Option<U>>
    where
        U: DeserializeOwned,
    {
        let content = self
            .csv_ds
            .get_file_content(&self.filename)
            .ok_or_else(|| DataSetError::other(format!("File '{}' not found", self.filename)))?;

        let mut reader = ReaderBuilder::new()
            .has_headers(true)
            .from_reader(content.as_bytes());

        let mut records = reader.deserialize();
        if let Some(result) = records.next() {
            let record: U = result
                .map_err(|e| DataSetError::other(format!("CSV deserialization error: {}", e)))?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }
}
