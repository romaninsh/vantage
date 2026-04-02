use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::Value;
use vantage_core::Result;
use vantage_core::util::error::Context;
use vantage_dataset::traits::{ReadableValueSet, ValueSet};
use vantage_types::Record;

pub struct CsvSource {
    path: String,
}

impl CsvSource {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }

    fn parse(&self) -> Result<IndexMap<usize, Record<Value>>> {
        let mut reader = csv::Reader::from_path(&self.path)
            .with_context(|| vantage_core::error!("Failed to open CSV file", path = &self.path))?;

        let headers = reader.headers()
            .context("Failed to read CSV headers")?
            .clone();

        let mut records = IndexMap::new();

        for (idx, result) in reader.records().enumerate() {
            let row = result.context("Failed to read CSV record")?;
            let mut record = Record::new();
            for (i, field) in row.iter().enumerate() {
                if let Some(header) = headers.get(i) {
                    record.insert(header.to_string(), Value::String(field.to_string()));
                }
            }
            records.insert(idx, record);
        }

        Ok(records)
    }
}

impl ValueSet for CsvSource {
    type Id = usize;
    type Value = Value;
}

#[async_trait]
impl ReadableValueSet for CsvSource {
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>> {
        self.parse()
    }

    async fn get_value(&self, id: &Self::Id) -> Result<Record<Self::Value>> {
        self.parse()?
            .shift_remove(id)
            .ok_or_else(|| vantage_core::error!("Record not found", id = id))
    }

    async fn get_some_value(&self) -> Result<Option<(Self::Id, Record<Self::Value>)>> {
        Ok(self.parse()?.into_iter().next())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(file: &str) -> String {
        format!("{}/data/{}", env!("CARGO_MANIFEST_DIR"), file)
    }

    #[tokio::test]
    async fn test_characters_count() {
        let source = CsvSource::new(data("characters.csv"));
        let records = source.list_values().await.unwrap();
        assert_eq!(records.len(), 10);
    }

    #[tokio::test]
    async fn test_characters_fields() {
        let source = CsvSource::new(data("characters.csv"));
        let records = source.list_values().await.unwrap();
        let marty = &records[&0];
        assert_eq!(marty["name"], Value::String("Marty McFly".to_string()));
        assert_eq!(marty["role"], Value::String("Protagonist".to_string()));
        assert_eq!(marty["hometown"], Value::String("Hill Valley".to_string()));
    }

    #[tokio::test]
    async fn test_get_some_value() {
        let source = CsvSource::new(data("characters.csv"));
        let (id, record) = source.get_some_value().await.unwrap().unwrap();
        assert_eq!(id, 0);
        assert_eq!(record["name"], Value::String("Marty McFly".to_string()));
    }

    #[tokio::test]
    async fn test_get_value_by_id() {
        let source = CsvSource::new(data("characters.csv"));
        let record = source.get_value(&2).await.unwrap();
        assert_eq!(record["name"], Value::String("Biff Tannen".to_string()));
    }

    #[tokio::test]
    async fn test_inventions_count() {
        let source = CsvSource::new(data("inventions.csv"));
        let records = source.list_values().await.unwrap();
        assert_eq!(records.len(), 10);
    }

    #[tokio::test]
    async fn test_inventions_fields() {
        let source = CsvSource::new(data("inventions.csv"));
        let records = source.list_values().await.unwrap();
        let delorean = &records[&0];
        assert_eq!(delorean["name"], Value::String("DeLorean Time Machine".to_string()));
        assert_eq!(delorean["inventor"], Value::String("Emmett Brown".to_string()));
        assert_eq!(delorean["status"], Value::String("Destroyed".to_string()));
    }

    #[tokio::test]
    async fn test_missing_file() {
        let source = CsvSource::new("/no/such/file.csv");
        assert!(source.list_values().await.is_err());
    }
}
