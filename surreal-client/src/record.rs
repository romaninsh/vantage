use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Represents a SurrealDB record ID
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecordId {
    pub table: String,
    pub id: RecordIdValue,
}

/// The value part of a record ID
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RecordIdValue {
    String(String),
    Number(i64),
    Object(Value),
    Array(Vec<Value>),
}

impl RecordId {
    /// Create a new record ID
    pub fn new(table: impl Into<String>, id: impl Into<RecordIdValue>) -> Self {
        Self {
            table: table.into(),
            id: id.into(),
        }
    }

    /// Create a record ID with a string ID
    pub fn string(table: impl Into<String>, id: impl Into<String>) -> Self {
        Self::new(table, RecordIdValue::String(id.into()))
    }

    /// Create a record ID with a numeric ID
    pub fn number(table: impl Into<String>, id: i64) -> Self {
        Self::new(table, RecordIdValue::Number(id))
    }

    /// Create a record ID with an object ID
    pub fn object(table: impl Into<String>, id: Value) -> Self {
        Self::new(table, RecordIdValue::Object(id))
    }

    /// Create a record ID with an array ID
    pub fn array(table: impl Into<String>, id: Vec<Value>) -> Self {
        Self::new(table, RecordIdValue::Array(id))
    }

    /// Parse a record ID from a string format "table:id"
    pub fn parse(input: &str) -> Result<Self, RecordParseError> {
        let parts: Vec<&str> = input.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(RecordParseError::InvalidFormat);
        }

        let table = parts[0].to_string();
        let id_str = parts[1];

        // Try to parse as number first
        if let Ok(num) = id_str.parse::<i64>() {
            return Ok(Self::number(table, num));
        }

        // Try to parse as JSON object/array
        if (id_str.starts_with('{') || id_str.starts_with('['))
            && let Ok(value) = serde_json::from_str::<Value>(id_str) { match value {
                Value::Object(_) => return Ok(Self::object(table, value)),
                Value::Array(arr) => return Ok(Self::array(table, arr)),
                _ => {}
            } }

        // Default to string
        Ok(Self::string(table, id_str))
    }

    /// Convert to SurrealQL string representation
    pub fn to_surql(&self) -> String {
        let table = escape_identifier(&self.table);
        let id = match &self.id {
            RecordIdValue::String(s) => escape_identifier(s),
            RecordIdValue::Number(n) => n.to_string(),
            RecordIdValue::Object(obj) => obj.to_string(),
            RecordIdValue::Array(arr) => {
                format!(
                    "[{}]",
                    arr.iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        };
        format!("{}:{}", table, id)
    }

    /// Get the table name
    pub fn table(&self) -> &str {
        &self.table
    }

    /// Get the ID value
    pub fn id(&self) -> &RecordIdValue {
        &self.id
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_surql())
    }
}

impl From<&str> for RecordId {
    fn from(s: &str) -> Self {
        Self::parse(s).unwrap_or_else(|_| Self::string("unknown", s))
    }
}

impl From<String> for RecordId {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

impl From<RecordId> for Value {
    fn from(record_id: RecordId) -> Self {
        Value::String(record_id.to_string())
    }
}

impl From<&RecordId> for Value {
    fn from(record_id: &RecordId) -> Self {
        Value::String(record_id.to_string())
    }
}

impl From<String> for RecordIdValue {
    fn from(s: String) -> Self {
        RecordIdValue::String(s)
    }
}

impl From<&str> for RecordIdValue {
    fn from(s: &str) -> Self {
        RecordIdValue::String(s.to_string())
    }
}

impl From<i64> for RecordIdValue {
    fn from(n: i64) -> Self {
        RecordIdValue::Number(n)
    }
}

impl From<Value> for RecordIdValue {
    fn from(v: Value) -> Self {
        match v {
            Value::String(s) => RecordIdValue::String(s),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    RecordIdValue::Number(i)
                } else {
                    RecordIdValue::Object(Value::Number(n))
                }
            }
            Value::Array(arr) => RecordIdValue::Array(arr),
            other => RecordIdValue::Object(other),
        }
    }
}

/// Table reference for queries
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
}

impl Table {
    /// Create a new table reference
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// Get the table name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Convert to SurrealQL string representation
    pub fn to_surql(&self) -> String {
        escape_identifier(&self.name)
    }
}

impl fmt::Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_surql())
    }
}

impl From<&str> for Table {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Table {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<Table> for Value {
    fn from(table: Table) -> Self {
        Value::String(table.name)
    }
}

impl From<&Table> for Value {
    fn from(table: &Table) -> Self {
        Value::String(table.name.clone())
    }
}

/// Record range for selecting multiple records
#[derive(Debug, Clone, PartialEq)]
pub struct RecordRange {
    pub table: String,
    pub start: Option<RecordIdValue>,
    pub end: Option<RecordIdValue>,
    pub start_inclusive: bool,
    pub end_inclusive: bool,
}

impl RecordRange {
    /// Create a new record range
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            start: None,
            end: None,
            start_inclusive: true,
            end_inclusive: true,
        }
    }

    /// Set the start of the range
    pub fn start(mut self, start: impl Into<RecordIdValue>, inclusive: bool) -> Self {
        self.start = Some(start.into());
        self.start_inclusive = inclusive;
        self
    }

    /// Set the end of the range
    pub fn end(mut self, end: impl Into<RecordIdValue>, inclusive: bool) -> Self {
        self.end = Some(end.into());
        self.end_inclusive = inclusive;
        self
    }

    /// Convert to SurrealQL string representation
    pub fn to_surql(&self) -> String {
        let table = escape_identifier(&self.table);

        let start_str = match &self.start {
            Some(start) => {
                let start_val = match start {
                    RecordIdValue::String(s) => escape_identifier(s),
                    RecordIdValue::Number(n) => n.to_string(),
                    RecordIdValue::Object(obj) => obj.to_string(),
                    RecordIdValue::Array(arr) => {
                        format!(
                            "[{}]",
                            arr.iter()
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                };
                if self.start_inclusive {
                    start_val
                } else {
                    format!(">{}", start_val)
                }
            }
            None => String::new(),
        };

        let end_str = match &self.end {
            Some(end) => {
                let end_val = match end {
                    RecordIdValue::String(s) => escape_identifier(s),
                    RecordIdValue::Number(n) => n.to_string(),
                    RecordIdValue::Object(obj) => obj.to_string(),
                    RecordIdValue::Array(arr) => {
                        format!(
                            "[{}]",
                            arr.iter()
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                };
                if self.end_inclusive {
                    end_val
                } else {
                    format!("={}", end_val)
                }
            }
            None => String::new(),
        };

        if start_str.is_empty() && end_str.is_empty() {
            table
        } else {
            format!("{}:{}..{}", table, start_str, end_str)
        }
    }
}

impl fmt::Display for RecordRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_surql())
    }
}

/// Error type for record ID parsing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordParseError {
    InvalidFormat,
    InvalidId,
}

impl fmt::Display for RecordParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordParseError::InvalidFormat => write!(f, "Invalid record ID format"),
            RecordParseError::InvalidId => write!(f, "Invalid record ID value"),
        }
    }
}

impl std::error::Error for RecordParseError {}

/// Escape a SurrealDB identifier if needed
fn escape_identifier(ident: &str) -> String {
    // Check if identifier needs escaping
    if ident.is_empty() {
        return "⟨⟩".to_string();
    }

    // Check if it's numeric
    if ident.parse::<i64>().is_ok() || ident.parse::<f64>().is_ok() {
        return format!("⟨{}⟩", ident);
    }

    // Check if it contains special characters or starts with a number
    if ident.chars().next().unwrap().is_ascii_digit()
        || ident.chars().any(|c| !c.is_alphanumeric() && c != '_')
    {
        return format!("⟨{}⟩", ident.replace('⟩', "\\⟩"));
    }

    ident.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_id_creation() {
        let record = RecordId::string("user", "john");
        assert_eq!(record.table, "user");
        assert_eq!(record.id, RecordIdValue::String("john".to_string()));
    }

    #[test]
    fn test_record_id_parsing() {
        let record = RecordId::parse("user:123").unwrap();
        assert_eq!(record.table, "user");
        assert_eq!(record.id, RecordIdValue::Number(123));

        let record = RecordId::parse("user:john").unwrap();
        assert_eq!(record.table, "user");
        assert_eq!(record.id, RecordIdValue::String("john".to_string()));
    }

    #[test]
    fn test_record_id_surql() {
        let record = RecordId::string("user", "john");
        assert_eq!(record.to_surql(), "user:john");

        let record = RecordId::number("user", 123);
        assert_eq!(record.to_surql(), "user:123");
    }

    #[test]
    fn test_table_creation() {
        let table = Table::new("users");
        assert_eq!(table.name(), "users");
        assert_eq!(table.to_surql(), "users");
    }

    #[test]
    fn test_escape_identifier() {
        assert_eq!(escape_identifier("normal"), "normal");
        assert_eq!(escape_identifier("123"), "⟨123⟩");
        assert_eq!(escape_identifier("with-dash"), "⟨with-dash⟩");
        assert_eq!(escape_identifier(""), "⟨⟩");
    }

    #[test]
    fn test_record_range() {
        let range = RecordRange::new("user").start("a", true).end("z", false);

        assert_eq!(range.to_surql(), "user:a..=z");
    }

    #[test]
    fn test_conversions() {
        let record = RecordId::string("user", "john");
        let value: Value = record.into();
        assert_eq!(value, Value::String("user:john".to_string()));

        let table = Table::new("users");
        let value: Value = table.into();
        assert_eq!(value, Value::String("users".to_string()));
    }
}
