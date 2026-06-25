//! Null detection and value equality for set-invariant enforcement.
//!
//! `vantage_table::Table` carries *invariants* — column values every row in a
//! set must hold (e.g. a has-many child's foreign key). Enforcing them on write
//! needs two operations on the backend's value type that the generic
//! `TableSource::Value` bound does not provide: tell whether a value is null,
//! and compare two values for equality. [`InvariantValue`] supplies both.
//!
//! Implementations live where the orphan rule allows: the macro-generated
//! `Any*Type` wrappers get one from [`crate::vantage_type_system`] (the wrapper
//! is local to the backend crate), and the raw representations used directly as
//! a `Value` (`serde_json::Value`, `ciborium::Value`) are implemented here.

/// A value type that can participate in set-invariant enforcement.
pub trait InvariantValue: Clone + Send + Sync + 'static {
    /// True only for a genuine null — `Option::None`, JSON/CBOR/BSON null,
    /// DynamoDB `{"NULL": true}`. Non-nullable representations (`String`,
    /// numbers) always return `false`; an empty string is not null.
    fn is_null(&self) -> bool;

    /// Equality against another value of the same type, compared on the
    /// underlying representation. Decides whether a written value already
    /// matches the set's invariant (keep) or conflicts with it (reject).
    fn value_eq(&self, other: &Self) -> bool;
}

#[cfg(feature = "serde")]
impl InvariantValue for serde_json::Value {
    fn is_null(&self) -> bool {
        matches!(self, serde_json::Value::Null)
    }
    fn value_eq(&self, other: &Self) -> bool {
        self == other
    }
}

impl InvariantValue for ciborium::Value {
    fn is_null(&self) -> bool {
        matches!(self, ciborium::Value::Null)
    }
    fn value_eq(&self, other: &Self) -> bool {
        self == other
    }
}
