//! CSV mock implementation for testing and examples

use crate::traits::{DataSet, ReadableDataSet, ReadableValueSet, Result, ValueSet};
use indexmap::IndexMap;
use std::collections::HashMap;
use vantage_core::util::error::{Context, vantage_error};
use vantage_types::{Entity, Record, vantage_type_system};

// CSV type system - everything is a string since CSV is text-based
vantage_type_system! {
    type_trait: CsvType,
    method_name: csv_string,
    value_type: String,
    type_variants: [String]
}

// Implement String for CSV type system
impl CsvType for String {
    type Target = CsvTypeStringMarker;

    fn to_csv_string(&self) -> String {
        self.clone()
    }

    fn from_csv_string(value: String) -> Option<Self> {
        Some(value)
    }
}

// Implement i64 for CSV type system (stored as string)
impl CsvType for i64 {
    type Target = CsvTypeStringMarker;

    fn to_csv_string(&self) -> String {
        self.to_string()
    }

    fn from_csv_string(value: String) -> Option<Self> {
        value.parse().ok()
    }
}

// Variant detection for CSV (only strings)
impl CsvTypeVariants {
    pub fn from_csv_string(_value: &String) -> Option<Self> {
        Some(CsvTypeVariants::String)
    }
}

/// MockCsv contains hardcoded CSV data as strings
#[derive(Debug, Clone)]
pub struct MockCsv {
    files: HashMap<String, String>,
}

impl Default for MockCsv {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCsv {
    pub fn new() -> Self {
        let mut files = HashMap::new();

        // Add users.csv data - all strings
        files.insert(
            "users.csv".to_string(),
            r#"id,name,email,age
1,Alice Johnson,alice@example.com,28
2,Bob Smith,bob@example.com,35
3,Charlie Brown,charlie@example.com,42
4,Diana Prince,diana@example.com,31"#
                .to_string(),
        );

        // Add products.csv data - all strings
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

    pub fn get_csv_file<T: Entity<AnyCsvType>>(&self, filename: &str) -> CsvFile<T> {
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
pub struct CsvFile<T: Entity<AnyCsvType>> {
    csv_ds: MockCsv,
    filename: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Entity<AnyCsvType>> ValueSet for CsvFile<T> {
    type Id = usize;
    type Value = AnyCsvType; // CSV values are always strings
}

impl<T: Entity<AnyCsvType>> CsvFile<T> {
    pub fn new(csv_ds: MockCsv, filename: &str) -> Self {
        Self {
            csv_ds,
            filename: filename.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<T> DataSet<T> for CsvFile<T> where T: Entity<AnyCsvType> {}

#[async_trait::async_trait]
impl<T> ReadableDataSet<T> for CsvFile<T>
where
    T: Entity<AnyCsvType>,
{
    async fn list(&self) -> Result<IndexMap<Self::Id, T>> {
        let values = self.list_values().await?;
        let mut records = IndexMap::new();

        for (id, record) in values {
            let entity = T::try_from_record(&record)
                .map_err(|_| vantage_error!("Failed to convert record to entity"))?;
            records.insert(id, entity);
        }

        Ok(records)
    }

    async fn get(&self, id: &Self::Id) -> Result<T> {
        let record = self.get_value(id).await?;
        let entity = T::try_from_record(&record)
            .map_err(|_| vantage_error!("Failed to convert record to entity"))?;
        Ok(entity)
    }

    async fn get_some(&self) -> Result<Option<(Self::Id, T)>> {
        if let Some((id, record)) = self.get_some_value().await? {
            let entity = T::try_from_record(&record)
                .map_err(|_| vantage_error!("Failed to convert record to entity"))?;
            Ok(Some((id, entity)))
        } else {
            Ok(None)
        }
    }
}

#[async_trait::async_trait]
impl<T> ReadableValueSet for CsvFile<T>
where
    T: Entity<AnyCsvType>,
{
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>> {
        let content = self
            .csv_ds
            .get_file_content(&self.filename)
            .context("Failed to get CSV content")?;

        let mut reader = csv::ReaderBuilder::new().from_reader(content.as_bytes());
        let mut records = IndexMap::new();

        let headers = reader
            .headers()
            .context("Failed to read CSV headers")?
            .clone();

        for (idx, result) in reader.records().enumerate() {
            let csv_record = result.context("Failed to read CSV record")?;

            // Convert CSV record to Record<AnyCsvType> (all CSV values are strings)
            let mut csv_record_map = Record::new();
            for (i, field) in csv_record.iter().enumerate() {
                if let Some(header) = headers.get(i) {
                    csv_record_map.insert(header.to_string(), AnyCsvType::new(field.to_string()));
                }
            }

            records.insert(idx, csv_record_map);
        }

        Ok(records)
    }

    async fn get_value(&self, id: &Self::Id) -> Result<Record<Self::Value>> {
        let content = self
            .csv_ds
            .get_file_content(&self.filename)
            .context("Failed to get CSV content")?;

        let mut reader = csv::ReaderBuilder::new().from_reader(content.as_bytes());

        let headers = reader
            .headers()
            .context("Failed to read CSV headers")?
            .clone();

        for (idx, result) in reader.records().enumerate() {
            if idx == *id {
                let csv_record = result.context("Failed to read CSV record")?;

                // Convert CSV record to Record<AnyCsvType>
                let mut csv_record_map = Record::new();
                for (i, field) in csv_record.iter().enumerate() {
                    if let Some(header) = headers.get(i) {
                        csv_record_map
                            .insert(header.to_string(), AnyCsvType::new(field.to_string()));
                    }
                }

                return Ok(csv_record_map);
            }
        }

        Err(vantage_error!("Record with index {} not found", id))
    }

    async fn get_some_value(&self) -> Result<Option<(Self::Id, Record<Self::Value>)>> {
        let values = self.list_values().await?;
        Ok(values.into_iter().next())
    }
}
