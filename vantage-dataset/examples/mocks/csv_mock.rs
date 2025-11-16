// examples/mocks/csv_mock.rs

use csv::ReaderBuilder;
use indexmap::IndexMap;
use std::collections::HashMap;
use vantage_core::util::error::{Context, vantage_error};
use vantage_dataset::traits::{DataSet, ReadableDataSet, ReadableValueSet, Result, ValueSet};
use vantage_types::{Entity, Record};

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

impl<T: Entity> ValueSet for CsvFile<T> {
    type Id = usize;
    type Value = serde_json::Value;
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
impl<T> DataSet<T> for CsvFile<T> where T: Entity {}

#[async_trait::async_trait]
impl<T> ReadableDataSet<T> for CsvFile<T>
where
    T: Entity,
{
    async fn list(&self) -> Result<IndexMap<Self::Id, T>> {
        let content = self
            .csv_ds
            .get_file_content(&self.filename)
            .context("Failed to get CSV content")?;

        let mut reader = ReaderBuilder::new().from_reader(content.as_bytes());
        let mut records = IndexMap::new();

        for (idx, result) in reader.deserialize::<T>().enumerate() {
            let record = result.context("Failed to deserialize CSV record")?;
            records.insert(idx, record);
        }

        Ok(records)
    }

    async fn get(&self, id: &Self::Id) -> Result<T> {
        let content = self
            .csv_ds
            .get_file_content(&self.filename)
            .context("Failed to get CSV content")?;

        let mut reader = ReaderBuilder::new().from_reader(content.as_bytes());

        for (idx, result) in reader.deserialize::<T>().enumerate() {
            if idx == *id {
                let record = result.context("Failed to deserialize CSV record")?;
                return Ok(record);
            }
        }

        Err(vantage_error!("Record with index {} not found", id))
    }

    async fn get_some(&self) -> Result<Option<(Self::Id, T)>> {
        let content = self
            .csv_ds
            .get_file_content(&self.filename)
            .context("Failed to get CSV content")?;

        let mut reader = ReaderBuilder::new().from_reader(content.as_bytes());

        if let Some(result) = reader.deserialize::<T>().next() {
            let record = result.context("Failed to deserialize CSV record")?;
            Ok(Some((0, record)))
        } else {
            Ok(None)
        }
    }
}

#[async_trait::async_trait]
impl<T> ReadableValueSet for CsvFile<T>
where
    T: Entity,
{
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>> {
        let content = self
            .csv_ds
            .get_file_content(&self.filename)
            .context("Failed to get CSV content")?;

        let mut reader = ReaderBuilder::new().from_reader(content.as_bytes());
        let mut records = IndexMap::new();

        for (idx, result) in reader.deserialize::<serde_json::Value>().enumerate() {
            let value = result.context("Failed to deserialize CSV record")?;
            let record: Record<serde_json::Value> = value.into();
            records.insert(idx, record);
        }

        Ok(records)
    }

    async fn get_value(&self, id: &Self::Id) -> Result<Record<Self::Value>> {
        let content = self
            .csv_ds
            .get_file_content(&self.filename)
            .context("Failed to get CSV content")?;

        let mut reader = ReaderBuilder::new().from_reader(content.as_bytes());

        for (idx, result) in reader.deserialize::<serde_json::Value>().enumerate() {
            if idx == *id {
                let value = result.context("Failed to deserialize CSV record")?;
                let record: Record<serde_json::Value> = value.into();
                return Ok(record);
            }
        }

        Err(vantage_error!("Record with index {} not found", id))
    }

    async fn get_some_value(&self) -> Result<Option<(Self::Id, Record<Self::Value>)>> {
        let values = self.list_values().await?;
        Ok(values.into_iter().next())
    }
}
