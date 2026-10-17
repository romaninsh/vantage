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

    /// Parse `"table:id"`. An integer id becomes a number, a JSON object or
    /// array id becomes that value, and anything else a string. Input without
    /// a `:` is [`RecordParseError::InvalidFormat`].
    pub fn parse(input: &str) -> Result<Self, RecordParseError> {
        let parts: Vec<&str> = input.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(RecordParseError::InvalidFormat);
        }

        let table = parts[0].to_string();
        let id_str = parts[1];

        if let Ok(num) = id_str.parse::<i64>() {
            return Ok(Self::number(table, num));
        }

        if (id_str.starts_with('{') || id_str.starts_with('['))
            && let Ok(value) = serde_json::from_str::<Value>(id_str)
        {
            match value {
                Value::Object(_) => return Ok(Self::object(table, value)),
                Value::Array(arr) => return Ok(Self::array(table, arr)),
                _ => {}
            }
        }

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

    /// Set the start of the range; `inclusive` keeps the start record itself
    pub fn start(mut self, start: impl Into<RecordIdValue>, inclusive: bool) -> Self {
        self.start = Some(start.into());
        self.start_inclusive = inclusive;
        self
    }

    /// Set the end of the range; `inclusive` keeps the end record itself
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

/// SurrealQL keywords that must be escaped when used as bare identifiers,
/// even though they are otherwise valid identifier characters.
const RESERVED_KEYWORDS: &[&str] = &[
    "DEFINE", "CREATE", "SELECT", "UPDATE", "DELETE", "FROM", "RETURN", "WHERE", "SET", "ONLY",
    "TABLE",
];

/// Escape the id half of a record id (`table:<id>`).
///
/// The same rules as [`escape_identifier`], except that an integer id passes
/// through unquoted: `t:1` is keyed by the integer `1`, while `t:⟨1⟩` is keyed
/// by the string `"1"`.
pub fn escape_record_id(id: &str) -> String {
    if !id.is_empty() && id.parse::<i64>().is_ok() {
        return id.to_string();
    }
    escape_identifier(id)
}

/// Escape a SurrealDB identifier so it is safe to embed in a query.
///
/// The crate's single escaping authority: record, table and range rendering
/// and `vantage-surrealdb`'s `Identifier` all route through it.
///
/// The identifier is wrapped in `⟨…⟩` when it is empty, numeric, starts with a
/// digit, contains a character outside `[A-Za-z0-9_]`, or is a reserved
/// keyword. Inside the brackets backslashes are doubled and `⟩` is written as
/// `\u{27E9}` (SurrealDB rejects `\⟩`), so a crafted name can't close the
/// quoting early. Plain identifiers pass through unchanged.
pub fn escape_identifier(ident: &str) -> String {
    if ident.is_empty() {
        return "⟨⟩".to_string();
    }

    // Numeric identifiers are always quoted (they can't contain `⟩`).
    if ident.parse::<i64>().is_ok() || ident.parse::<f64>().is_ok() {
        return format!("⟨{}⟩", ident);
    }

    let is_reserved = RESERVED_KEYWORDS.contains(&ident.to_uppercase().as_str());

    if is_reserved
        || ident.chars().next().unwrap().is_ascii_digit()
        || ident.chars().any(|c| !c.is_alphanumeric() && c != '_')
    {
        // Backslash first, so the `\u{27E9}` we emit for `⟩` isn't re-escaped.
        let escaped = ident.replace('\\', "\\\\").replace('⟩', "\\u{27E9}");
        return format!("⟨{}⟩", escaped);
    }

    ident.to_string()
}
