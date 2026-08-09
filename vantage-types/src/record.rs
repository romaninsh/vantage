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

    /// Create a record from an IndexMap
    pub fn from_indexmap(map: IndexMap<String, V>) -> Self {
        Self { inner: map }
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

impl<V: PartialEq + Clone> Record<V> {
    /// The fields of `self` that `before` does not already hold with the
    /// same value — the partial record that turns `before` into `self`.
    ///
    /// Write this instead of the whole entity. A whole-entity write
    /// carries every field the caller read, so it overwrites a field
    /// another writer changed in between, and it saves back values this
    /// caller never looked at — including ones the entity failed to
    /// parse and filled with a default.
    ///
    /// Fields present in `before` and absent from `self` are NOT
    /// included: the result is a merge, and a merge cannot express a
    /// removal. Set the field to the representation's null instead.
    pub fn changes_from(&self, before: &Record<V>) -> Record<V> {
        let mut out = Record::new();
        for (key, value) in &self.inner {
            if before.inner.get(key) != Some(value) {
                out.inner.insert(key.clone(), value.clone());
            }
        }
        out
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
impl From<Record<serde_json::Value>> for serde_json::Value {
    fn from(val: Record<serde_json::Value>) -> Self {
        let map: serde_json::Map<String, serde_json::Value> = val.inner.into_iter().collect();
        serde_json::Value::Object(map)
    }
}

// Direct conversion from ciborium::Value to Record — mirrors the JSON
// impl above. Non-Map values are wrapped under a `"value"` key.
impl From<ciborium::Value> for Record<ciborium::Value> {
    fn from(value: ciborium::Value) -> Self {
        match value {
            ciborium::Value::Map(pairs) => pairs
                .into_iter()
                .filter_map(|(k, v)| match k {
                    ciborium::Value::Text(s) => Some((s, v)),
                    _ => None,
                })
                .collect(),
            _ => {
                let mut record = Record::new();
                record.insert("value".to_string(), value);
                record
            }
        }
    }
}

// Reverse conversion from Record to ciborium::Value (always a Map).
impl From<Record<ciborium::Value>> for ciborium::Value {
    fn from(val: Record<ciborium::Value>) -> Self {
        ciborium::Value::Map(
            val.inner
                .into_iter()
                .map(|(k, v)| (ciborium::Value::Text(k), v))
                .collect(),
        )
    }
}

// Clean record conversion traits
pub trait IntoRecord<T> {
    /// Infallibly convert `self` into a [`Record`].
    ///
    /// This covers conversions that cannot fail: type-system reshaping between
    /// value representations (`Record<T>` → `Record<U>`) and the field-by-field
    /// impls generated by the `#[entity]` macro. Serializing an arbitrary serde
    /// entity *can* fail, so that path lives on [`TryIntoRecord`] — the fallible
    /// counterpart to [`TryFromRecord`] — and is not reachable through this trait.
    fn into_record(self) -> Record<T>;
}

/// Fallible conversion of `self` into a [`Record`], the mirror of [`TryFromRecord`].
///
/// Serializing a user entity into a record can legitimately fail — a `HashMap`
/// with non-string keys under JSON, a non-text map key under CBOR, an
/// out-of-range number, or a hand-written `Serialize` that returns an error.
/// Returning `Result` keeps such failures recoverable instead of aborting the
/// process deep inside a write path.
pub trait TryIntoRecord<T> {
    type Error;
    fn try_into_record(self) -> Result<Record<T>, Self::Error>;
}

pub trait TryFromRecord<T: Clone>: Sized {
    type Error;
    fn from_record(record: Record<T>) -> Result<Self, Self::Error>;
    fn try_from_record(record: &Record<T>) -> Result<Self, Self::Error> {
        Self::from_record(record.clone())
    }
}

// Blanket implementations for Record conversion
impl<T, U> IntoRecord<U> for Record<T>
where
    T: Into<U>,
{
    fn into_record(self) -> Record<U> {
        self.into_iter().map(|(k, v)| (k, v.into())).collect()
    }
}

impl<T, U> TryIntoRecord<U> for Record<T>
where
    T: Into<U>,
{
    type Error = std::convert::Infallible;

    fn try_into_record(self) -> Result<Record<U>, Self::Error> {
        Ok(self.into_iter().map(|(k, v)| (k, v.into())).collect())
    }
}

impl<T, U> TryFromRecord<U> for Record<T>
where
    T: TryFrom<U>,
    U: Clone,
{
    type Error = T::Error;

    fn from_record(record: Record<U>) -> Result<Self, Self::Error> {
        let mut result = Record::new();
        for (key, value) in record.into_iter() {
            let converted_value = T::try_from(value)?;
            result.insert(key, converted_value);
        }
        Ok(result)
    }
}

// Blanket implementations for serde types
#[cfg(feature = "serde")]
impl<T> TryIntoRecord<serde_json::Value> for T
where
    T: serde::Serialize,
{
    type Error = serde_json::Error;

    fn try_into_record(self) -> Result<Record<serde_json::Value>, Self::Error> {
        let json_value = serde_json::to_value(self)?;

        Ok(match json_value {
            serde_json::Value::Object(map) => map.into_iter().collect(),
            _ => {
                // Handle non-object values by wrapping them
                let mut record = Record::new();
                record.insert("value".to_string(), json_value);
                record
            }
        })
    }
}

#[cfg(feature = "serde")]
impl<T> TryFromRecord<serde_json::Value> for T
where
    T: serde::de::DeserializeOwned,
{
    type Error = serde_json::Error;

    fn from_record(record: Record<serde_json::Value>) -> Result<Self, Self::Error> {
        // Convert Record to JSON object
        let json_object: serde_json::Map<String, serde_json::Value> =
            record.inner.into_iter().collect();

        let json_value = serde_json::Value::Object(json_object);
        serde_json::from_value(json_value)
    }
}

// Blanket impls bridging serde-derived entity structs to ciborium::Value.
// `vantage_vista::Vista` carries `Record<ciborium::Value>` across the
// type-erased boundary, so any entity wrapped through a driver's
// `vista_factory().from_table(...)` needs `Entity<ciborium::Value>`.
// These impls auto-supply that for any Serialize/DeserializeOwned
// struct, mirroring the JSON impls above.
#[cfg(feature = "serde")]
impl<T> TryIntoRecord<ciborium::Value> for T
where
    T: serde::Serialize,
{
    type Error = ciborium::value::Error;

    fn try_into_record(self) -> Result<Record<ciborium::Value>, Self::Error> {
        use serde::ser::Error as _;

        let cbor_value = ciborium::Value::serialized(&self)?;

        Ok(match cbor_value {
            ciborium::Value::Map(pairs) => {
                let mut record = Record::new();
                for (k, v) in pairs {
                    match k {
                        ciborium::Value::Text(s) => {
                            record.insert(s, v);
                        }
                        other => {
                            // A non-text key cannot be a record field name.
                            // Surface it rather than dropping the entry silently.
                            return Err(ciborium::value::Error::custom(format!(
                                "CBOR map key is not text: {other:?}"
                            )));
                        }
                    }
                }
                record
            }
            _ => {
                let mut record = Record::new();
                record.insert("value".to_string(), cbor_value);
                record
            }
        })
    }
}

#[cfg(feature = "serde")]
impl<T> TryFromRecord<ciborium::Value> for T
where
    T: serde::de::DeserializeOwned,
{
    type Error = ciborium::value::Error;

    fn from_record(record: Record<ciborium::Value>) -> Result<Self, Self::Error> {
        let cbor_map: Vec<(ciborium::Value, ciborium::Value)> = record
            .inner
            .into_iter()
            .map(|(k, v)| (ciborium::Value::Text(k), v))
            .collect();

        let cbor_value = ciborium::Value::Map(cbor_map);
        cbor_value.deserialized()
    }
}

#[cfg(test)]
mod changes_from_tests {
    use super::Record;

    fn record(pairs: &[(&str, &str)]) -> Record<String> {
        let mut r = Record::new();
        for (k, v) in pairs {
            r.insert(k.to_string(), v.to_string());
        }
        r
    }

    #[test]
    fn only_the_differing_fields_come_back() {
        let before = record(&[("status", "unregistered"), ("found", "[]")]);
        let after = record(&[("status", "pending"), ("found", "[]")]);

        let changes = after.changes_from(&before);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes.get("status").map(String::as_str), Some("pending"));
        // The whole point: an untouched field is not written back, so a
        // concurrent writer's value survives.
        assert!(!changes.contains_key("found"));
    }

    #[test]
    fn a_field_the_earlier_record_lacks_counts_as_a_change() {
        let before = record(&[("status", "unregistered")]);
        let after = record(&[("status", "unregistered"), ("verification_code", "123456")]);

        let changes = after.changes_from(&before);

        assert_eq!(changes.len(), 1);
        assert!(changes.contains_key("verification_code"));
    }

    #[test]
    fn identical_records_produce_nothing() {
        let same = record(&[("status", "registered")]);
        assert!(same.changes_from(&same).is_empty());
    }

    /// A merge cannot express a removal, so a field that disappeared is
    /// not reported — callers set the representation's null instead.
    #[test]
    fn a_dropped_field_is_not_reported() {
        let before = record(&[("status", "pending"), ("verification_code", "123456")]);
        let after = record(&[("status", "registered")]);

        let changes = after.changes_from(&before);

        assert_eq!(changes.len(), 1);
        assert!(changes.contains_key("status"));
        assert!(!changes.contains_key("verification_code"));
    }
}
