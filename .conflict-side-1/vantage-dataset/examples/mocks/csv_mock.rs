// examples/mocks/csv_mock.rs

use csv::ReaderBuilder;
use std::collections::HashMap;
use vantage_core::Entity;
use vantage_core::util::error::{Context, vantage_error};
use vantage_dataset::dataset::{
    Id, ReadableAsDataSet, ReadableDataSet, ReadableValueSet, Result, VantageError,
};

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
            r#"id,name,email,age
1,Alice Johnson,alice@example.com,28
2,Bob Smith,bob@example.com,35
3,Charlie Brown,charlie@example.com,42
4,Diana Prince,diana@example.com,31"#
                .to_string(),
        );

        // Add products.csv data
        files.insert(
            "products.csv".to_string(),
            r#"id,name,price,category
101,Laptop,999.99,Electronics
102,Coffee Mug,12.50,Kitchen
103,Notebook,5.99,Office
104,Wireless Mouse,25.00,Electronics"#
                .to_string(),
        );

        Self { files }
    }

    pub fn get_csv_file<T: Entity>(&self, filename: &str) -> CsvFile<T> {
        CsvFile::new(self.clone(), filename)
    }

    pub fn list_files(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }

    pub fn get_file_content(&self, filename: &str) -> Result<&str> {
        self.files
            .get(filename)
            .map(|s| s.as_str())
            .ok_or_else(|| vantage_error!("File {} not found", filename))
    }
}

/// CsvFile represents a single CSV file that can be read as a dataset
#[derive(Debug, Clone)]
pub struct CsvFile<T: Entity> {
    csv_ds: MockCsv,
    filename: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Entity> CsvFile<T> {
    pub fn new(csv_ds: MockCsv, filename: &str) -> Self {
        Self {
            csv_ds,
            filename: filename.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<T> ReadableDataSet<T> for CsvFile<T>
where
    T: Entity,
{
    async fn get(&self) -> Result<Vec<T>> {
        self.get_as().await
    }

    async fn get_id(&self, _id: impl Id) -> Result<T> {
        return Err(VantageError::no_capability("get_id", "CsvFile"));
    }

    async fn get_some(&self) -> Result<Option<T>> {
        self.get_some_as().await
    }
}

#[async_trait::async_trait]
impl<T> ReadableValueSet for CsvFile<T>
where
    T: Entity,
{
    async fn get_values(&self) -> Result<Vec<serde_json::Value>> {
        let content = self
            .csv_ds
            .get_file_content(&self.filename)
            .context("Failed to get CSV content")?;

        let mut reader = ReaderBuilder::new().from_reader(content.as_bytes());
        let mut records = Vec::new();

        for result in reader.deserialize::<serde_json::Value>() {
            let record = result.context("Failed to deserialize CSV record")?;
            records.push(record);
        }

        Ok(records)
    }

    async fn get_id_value(&self, _id: &str) -> Result<serde_json::Value> {
        return Err(VantageError::no_capability("get_id_value", "CsvFile"));
    }

    async fn get_some_value(&self) -> Result<Option<serde_json::Value>> {
        let values = self.get_values().await?;
        Ok(values.into_iter().next())
    }
}

#[async_trait::async_trait]
impl<T> ReadableAsDataSet for CsvFile<T>
where
    T: Entity,
{
    async fn get_as<U>(&self) -> Result<Vec<U>>
    where
        U: Entity,
    {
        let content = self
            .csv_ds
            .get_file_content(&self.filename)
            .context("Failed to get CSV content")?;

        let mut reader = ReaderBuilder::new().from_reader(content.as_bytes());
        let mut records = Vec::new();

        for result in reader.deserialize::<U>() {
            let record = result.context("Failed to deserialize CSV record")?;
            records.push(record);
        }

        Ok(records)
    }

    async fn get_id_as<U>(&self, _id: &str) -> Result<U>
    where
        U: Entity,
    {
        return Err(VantageError::no_capability("get_id_as", "CsvFile"));
    }

    async fn get_some_as<U>(&self) -> Result<Option<U>>
    where
        U: Entity,
    {
        let records = self.get_as().await?;
        Ok(records.into_iter().next())
    }
}
