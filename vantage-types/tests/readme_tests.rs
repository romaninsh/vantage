use indexmap::IndexMap;

use vantage_types::vantage_type_system;
use vantage_types_entity::entity;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Email {
    pub name: String,
    pub domain: String,
}

impl Email {
    pub fn new(name: &str, domain: &str) -> Self {
        Self {
            name: name.to_string(),
            domain: domain.to_string(),
        }
    }
}

impl std::fmt::Display for Email {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_csv_examples() {
        // CSV persistence engine example
        vantage_type_system! {
            type_trait: CsvType,
            method_name: csv_string,
            value_type: String,
            type_variants: [Text]
        }
        use vantage_core::{Context, Result};
        use vantage_types::{IntoRecord, Record, TryFromRecord};

        impl CsvType for String {
            type Target = CsvTypeTextMarker;

            fn to_csv_string(&self) -> String {
                self.clone()
            }

            fn from_csv_string(value: String) -> Option<Self> {
                Some(value)
            }
        }

        impl CsvType for Email {
            type Target = CsvTypeTextMarker;

            fn to_csv_string(&self) -> String {
                format!("{}@{}", self.name, self.domain)
            }

            fn from_csv_string(value: String) -> Option<Self> {
                let parts: Vec<&str> = value.split('@').collect();
                if parts.len() == 2 {
                    Some(Email {
                        name: parts[0].to_string(),
                        domain: parts[1].to_string(),
                    })
                } else {
                    None
                }
            }
        }

        impl CsvTypeVariants {
            pub fn from_csv_string(_value: &String) -> Option<Self> {
                Some(CsvTypeVariants::Text)
            }
        }

        #[derive(Debug, PartialEq)]
        #[entity(CsvType)]
        struct User {
            name: String,
            email: Email,
        }
        // Layer 1: Low-level persistence operations (hardcoded for testing)
        async fn actually_read_csv_contents()
        -> std::result::Result<Vec<IndexMap<String, String>>, std::io::Error> {
            let mut result = Vec::new();

            let mut record1 = IndexMap::new();
            record1.insert("name".to_string(), "Alice".to_string());
            record1.insert("email".to_string(), "alice@paris.com".to_string());

            let mut record2 = IndexMap::new();
            record2.insert("name".to_string(), "Bob".to_string());
            record2.insert("email".to_string(), "bob@london.com".to_string());

            result.push(record1);
            result.push(record2);

            Ok(result)
        }

        async fn actually_insert_csv_record(
            _data: IndexMap<String, String>,
        ) -> std::result::Result<(), std::io::Error> {
            // In real implementation, this would write to CSV file
            Ok(())
        }

        // Layer 2: Vantage type system integration
        async fn read_csv_contents() -> Result<Vec<Record<AnyCsvType>>> {
            // convert error into VantageError
            let contents = actually_read_csv_contents()
                .await
                .context("Failed to read CSV contents")?;

            // Convert each row into Record<AnyCsvType>
            Ok(contents
                .into_iter()
                .map(|row| {
                    let record = Record::from_indexmap(row); // Record<String>
                    Record::<AnyCsvType>::try_from_record(&record).unwrap() // convert to Record<AnyCsvType>
                })
                .collect())
        }

        async fn insert_csv_record<T>(record: T) -> Result<()>
        where
            T: IntoRecord<AnyCsvType>,
        {
            let vantage_record = record.into_record();

            // Convert to underlying value type
            let string_record: Record<String> = vantage_record.into_record();

            // Convert Record<AnyCsvType> into IndexMap<String, String>
            let indexmap = string_record.into_inner();

            // Convert error into VantageError
            actually_insert_csv_record(indexmap)
                .await
                .context("Failed to insert CSV record")
        }

        let _ = insert_csv_record(User {
            name: "John Doe".to_string(),
            email: Email::new("john", "example.com"),
        })
        .await;

        let records = read_csv_contents().await.unwrap();
        for record in records {
            let user: User = User::try_from_record(&record).unwrap();
            println!("User: {} ({})", user.name, user.email);

            // alternatively:
            println!(
                "User: {} ({})",
                record["name"].value(),
                record["email"].value()
            );
        }
    }
}
