use bson::Bson;
use serde_json::Value as JsonValue;
use vantage_csv::{CsvType, type_system::CsvTypeAnimalMarker};
use vantage_mongodb::types::MongoTypeStringMarker;
use vantage_mongodb::MongoType;
use vantage_sql::postgres::{PostgresType, types::PostgresTypeTextMarker};
use vantage_sql::sqlite::{SqliteType, types::SqliteTypeTextMarker};
use vantage_surrealdb::types::SurrealTypeStringMarker;
use vantage_surrealdb::{CborValue, SurrealType};
use vantage_types::TerminalRender;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Animal {
    #[default]
    Cat,
    Dog,
    Pig,
    Cow,
    Chicken,
}

impl std::str::FromStr for Animal {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "cat" => Ok(Animal::Cat),
            "dog" => Ok(Animal::Dog),
            "pig" => Ok(Animal::Pig),
            "cow" => Ok(Animal::Cow),
            "chicken" => Ok(Animal::Chicken),
            _ => Err(format!("Unknown animal: {}", s)),
        }
    }
}

impl Animal {
    pub fn as_str(&self) -> &'static str {
        match self {
            Animal::Cat => "cat",
            Animal::Dog => "dog",
            Animal::Pig => "pig",
            Animal::Cow => "cow",
            Animal::Chicken => "chicken",
        }
    }
}

impl TerminalRender for Animal {
    fn render(&self) -> String {
        match self {
            Animal::Cat => "🐱".to_string(),
            Animal::Dog => "🐶".to_string(),
            Animal::Pig => "🐷".to_string(),
            Animal::Cow => "🐮".to_string(),
            Animal::Chicken => "🐔".to_string(),
        }
    }
}

impl CsvType for Animal {
    type Target = CsvTypeAnimalMarker;

    fn to_csv_string(&self) -> String {
        self.as_str().to_string()
    }

    fn from_csv_string(value: String) -> Option<Self> {
        value.parse().ok()
    }
}

impl SqliteType for Animal {
    type Target = SqliteTypeTextMarker;

    fn to_json(&self) -> JsonValue {
        JsonValue::String(self.as_str().to_string())
    }

    fn from_json(value: JsonValue) -> Option<Self> {
        match value {
            JsonValue::String(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl PostgresType for Animal {
    type Target = PostgresTypeTextMarker;

    fn to_json(&self) -> JsonValue {
        JsonValue::String(self.as_str().to_string())
    }

    fn from_json(value: JsonValue) -> Option<Self> {
        match value {
            JsonValue::String(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl SurrealType for Animal {
    type Target = SurrealTypeStringMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Text(self.as_str().to_string())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl MongoType for Animal {
    type Target = MongoTypeStringMarker;

    fn to_bson(&self) -> Bson {
        Bson::String(self.as_str().to_string())
    }

    fn from_bson(value: Bson) -> Option<Self> {
        match value {
            Bson::String(s) => s.parse().ok(),
            _ => None,
        }
    }
}
