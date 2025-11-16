use indexmap::IndexMap;
use std::ops::{Deref, DerefMut};

/// A record is a key-value mapping where keys are field names and values are of type V.
///
/// This struct wraps IndexMap to provide a convenient way to represent structured data records
/// that maintain field insertion order, which is useful for consistent serialization
/// and display purposes.
///
/// # Type Parameters
///
/// - `V`: The value type for record fields, typically `serde_json::Value` or similar
///
/// # Examples
///
/// ```rust
/// use vantage_types::Record;
/// use serde_json::Value;
///
/// let mut user_record: Record<Value> = Record::new();
/// user_record.insert("name".to_string(), Value::String("Alice".to_string()));
/// user_record.insert("age".to_string(), Value::Number(30.into()));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record<V> {
    inner: IndexMap<String, V>,
}

impl<V> Record<V> {
    /// Create a new empty record
    pub fn new() -> Self {
        Self {
            inner: IndexMap::new(),
        }
    }

    /// Create a new record with the specified capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: IndexMap::with_capacity(capacity),
        }
    }

    /// Convert into the underlying IndexMap
    pub fn into_inner(self) -> IndexMap<String, V> {
        self.inner
    }

    /// Get a reference to the underlying IndexMap
    pub fn as_inner(&self) -> &IndexMap<String, V> {
        &self.inner
    }

    /// Get a mutable reference to the underlying IndexMap
    pub fn as_inner_mut(&mut self) -> &mut IndexMap<String, V> {
        &mut self.inner
    }
}

impl<V> Default for Record<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> Deref for Record<V> {
    type Target = IndexMap<String, V>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<V> DerefMut for Record<V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<V> From<IndexMap<String, V>> for Record<V> {
    fn from(inner: IndexMap<String, V>) -> Self {
        Self { inner }
    }
}

impl<V> From<Record<V>> for IndexMap<String, V> {
    fn from(record: Record<V>) -> Self {
        record.inner
    }
}

impl<V> FromIterator<(String, V)> for Record<V> {
    fn from_iter<T: IntoIterator<Item = (String, V)>>(iter: T) -> Self {
        Self {
            inner: IndexMap::from_iter(iter),
        }
    }
}

impl<V> IntoIterator for Record<V> {
    type Item = (String, V);
    type IntoIter = indexmap::map::IntoIter<String, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a, V> IntoIterator for &'a Record<V> {
    type Item = (&'a String, &'a V);
    type IntoIter = indexmap::map::Iter<'a, String, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl<'a, V> IntoIterator for &'a mut Record<V> {
    type Item = (&'a String, &'a mut V);
    type IntoIter = indexmap::map::IterMut<'a, String, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter_mut()
    }
}

// Serde compatibility for Record<serde_json::Value>
#[cfg(feature = "serde")]
impl Record<serde_json::Value> {
    /// Convert a serializable type into a Record
    pub fn from_serializable<T: serde::Serialize>(value: T) -> Result<Self, serde_json::Error> {
        let json_value = serde_json::to_value(value)?;

        match json_value {
            serde_json::Value::Object(map) => Ok(map.into_iter().collect()),
            _ => {
                // Handle non-object values by wrapping them
                let mut record = Record::new();
                record.insert("value".to_string(), json_value);
                Ok(record)
            }
        }
    }

    /// Convert Record to a deserializable type
    pub fn to_deserializable<T: serde::de::DeserializeOwned>(
        &self,
    ) -> Result<T, serde_json::Error> {
        // Convert Record to JSON object
        let json_object: serde_json::Map<String, serde_json::Value> = self
            .inner
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let json_value = serde_json::Value::Object(json_object);
        serde_json::from_value(json_value)
    }
}

// Direct conversion from serde_json::Value to Record
#[cfg(feature = "serde")]
impl From<serde_json::Value> for Record<serde_json::Value> {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Object(map) => map.into_iter().collect(),
            _ => {
                // Handle non-object values by wrapping them
                let mut record = Record::new();
                record.insert("value".to_string(), value);
                record
            }
        }
    }
}

// Reverse conversion from Record to serde_json::Value
#[cfg(feature = "serde")]
impl Into<serde_json::Value> for Record<serde_json::Value> {
    fn into(self) -> serde_json::Value {
        let map: serde_json::Map<String, serde_json::Value> = self.inner.into_iter().collect();
        serde_json::Value::Object(map)
    }
}
