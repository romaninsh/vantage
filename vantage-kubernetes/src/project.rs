//! Projection: a raw Kubernetes object → a flat `Record<CborValue>`.
//!
//! K8s objects are deeply nested and pack key data into arrays. Vantage
//! columns, relations, and charts all want flat, typed fields. Each model
//! module owns a projector that builds a [`Row`] for its resource using
//! the helpers here:
//!
//! - [`dig`] / [`str_at`] / [`int_at`] — walk a dotted path into nested
//!   objects (`status.phase`, `spec.nodeName`),
//! - [`label`] — read a single label/annotation key verbatim (label keys
//!   contain dots and slashes, so they can't go through `dig`),
//! - [`Row`] — accumulate `(column, value)` pairs and parse quantities.

use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_types::Record;

use crate::types::quantity;

/// Walk a dotted path through nested JSON objects. Each segment is an
/// object key; returns `None` if any segment is missing. Use [`label`] for
/// label/annotation maps whose keys themselves contain dots.
pub fn dig<'a>(item: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let mut cur = item;
    for part in path.split('.') {
        cur = cur.get(part)?;
    }
    Some(cur)
}

/// String value at a dotted path.
pub fn str_at(item: &JsonValue, path: &str) -> Option<String> {
    dig(item, path).and_then(|v| v.as_str()).map(str::to_string)
}

/// Integer value at a dotted path (accepts JSON numbers).
pub fn int_at(item: &JsonValue, path: &str) -> Option<i64> {
    dig(item, path).and_then(|v| v.as_i64())
}

/// Read a single label by key from `metadata.labels`. Label keys legally
/// contain `.` and `/` (e.g. `kubernetes.io/hostname`), so this does a
/// direct map lookup rather than a dotted-path walk.
pub fn label(item: &JsonValue, key: &str) -> Option<String> {
    item.get("metadata")?
        .get("labels")?
        .get(key)?
        .as_str()
        .map(str::to_string)
}

/// The owning workload's name from `metadata.ownerReferences[0]`, plus its
/// kind. Returns `(name, kind)`.
pub fn owner(item: &JsonValue) -> (Option<String>, Option<String>) {
    let Some(first) = item
        .get("metadata")
        .and_then(|m| m.get("ownerReferences"))
        .and_then(|o| o.as_array())
        .and_then(|a| a.first())
    else {
        return (None, None);
    };
    let name = first.get("name").and_then(|v| v.as_str()).map(str::to_string);
    let kind = first.get("kind").and_then(|v| v.as_str()).map(str::to_string);
    (name, kind)
}

/// Recover the owning Deployment's name from a ReplicaSet-owned Pod. A
/// Deployment names its ReplicaSets `<deployment>-<pod-template-hash>`, so
/// stripping the final `-<hash>` segment yields the Deployment name. For
/// pods not owned by a ReplicaSet, the owner name is returned unchanged.
pub fn owner_deployment(item: &JsonValue) -> Option<String> {
    let (name, kind) = owner(item);
    let name = name?;
    match kind.as_deref() {
        Some("ReplicaSet") => Some(name.rsplit_once('-').map(|(head, _)| head).unwrap_or(&name).to_string()),
        _ => Some(name),
    }
}

/// `kubectl`-style age relative to now, from `metadata.creationTimestamp`.
/// Uses the wall clock, so it's intentionally not part of the pure-path
/// helpers above (projector tests assert the stable fields instead).
pub fn age(item: &JsonValue) -> Option<String> {
    let ts = str_at(item, "metadata.creationTimestamp")?;
    crate::types::datetime::age_from(&ts, chrono::Utc::now())
}

/// Accumulates a flat record for one resource. Column order is preserved.
pub struct Row {
    fields: Vec<(String, CborValue)>,
}

impl Row {
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    /// Set a column to a raw CBOR value.
    pub fn set(mut self, col: &str, value: CborValue) -> Self {
        self.fields.push((col.to_string(), value));
        self
    }

    /// Copy a string field from a dotted path (omitted if missing).
    pub fn str(self, col: &str, item: &JsonValue, path: &str) -> Self {
        match str_at(item, path) {
            Some(s) => self.set(col, CborValue::Text(s)),
            None => self,
        }
    }

    /// Copy an integer field from a dotted path (omitted if missing).
    pub fn int(self, col: &str, item: &JsonValue, path: &str) -> Self {
        match int_at(item, path) {
            Some(n) => self.set(col, CborValue::from(n)),
            None => self,
        }
    }

    /// Copy a boolean field from a dotted path (omitted if missing).
    #[allow(dead_code)]
    pub fn bool(self, col: &str, item: &JsonValue, path: &str) -> Self {
        match dig(item, path).and_then(|v| v.as_bool()) {
            Some(b) => self.set(col, CborValue::Bool(b)),
            None => self,
        }
    }

    /// Set a text column from an owned `String`.
    pub fn text(self, col: &str, value: impl Into<String>) -> Self {
        self.set(col, CborValue::Text(value.into()))
    }

    /// Set an optional text column (omitted if `None`).
    pub fn opt_text(self, col: &str, value: Option<String>) -> Self {
        match value {
            Some(s) => self.text(col, s),
            None => self,
        }
    }

    /// Set an integer column.
    pub fn num(self, col: &str, value: i64) -> Self {
        self.set(col, CborValue::from(value))
    }

    /// Parse a CPU quantity at `path` into millicores under `col`.
    pub fn cpu_millicores(self, col: &str, item: &JsonValue, path: &str) -> Self {
        match str_at(item, path).as_deref().and_then(quantity::parse_cpu_millicores) {
            Some(n) => self.num(col, n),
            None => self,
        }
    }

    /// Parse a memory/storage quantity at `path` into bytes under `col`.
    pub fn memory_bytes(self, col: &str, item: &JsonValue, path: &str) -> Self {
        match str_at(item, path).as_deref().and_then(quantity::parse_memory_bytes) {
            Some(n) => self.num(col, n),
            None => self,
        }
    }

    pub fn build(self) -> Record<CborValue> {
        self.fields.into_iter().collect()
    }
}

impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}
