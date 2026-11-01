//! [`Denials`]: the writes a rule forbids on a table, and [`forbidden`] /
//! [`forbidden_column`], the refusals that name the rule.

use indexmap::IndexMap;
use vantage_core::{Result, VantageError, error};

use crate::capabilities::{Capability, VistaCapabilities};

/// Capabilities and columns a rule has denied, each with the rule that
/// denied it.
///
/// A denied capability is refused `Forbidden` whatever the shell advertises;
/// a denied column is read-only: a `patch` that sets it is refused, a whole-row
/// `replace` or `upsert` keeps its stored value whatever the record carries.
/// Denials only accumulate: [`merge`](Self::merge)
/// adds, nothing removes, and the first rule given for an entry is kept.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Denials {
    capabilities: IndexMap<Capability, String>,
    columns: IndexMap<String, String>,
}

impl Denials {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn deny(&mut self, capability: Capability, rule: impl Into<String>) {
        self.capabilities
            .entry(capability)
            .or_insert_with(|| rule.into());
    }

    pub fn deny_column(&mut self, column: impl Into<String>, rule: impl Into<String>) {
        self.columns
            .entry(column.into())
            .or_insert_with(|| rule.into());
    }

    /// The rule that denied `capability`, if one did.
    pub fn rule_for(&self, capability: Capability) -> Option<&str> {
        self.capabilities.get(&capability).map(String::as_str)
    }

    /// The rule that made `column` read-only, if one did.
    pub fn column_rule(&self, column: &str) -> Option<&str> {
        self.columns.get(column).map(String::as_str)
    }

    /// The read-only columns, each with the rule that denied it.
    pub fn columns(&self) -> impl Iterator<Item = (&str, &str)> {
        self.columns.iter().map(|(c, r)| (c.as_str(), r.as_str()))
    }

    /// The denied capabilities as a set.
    pub fn capabilities(&self) -> VistaCapabilities {
        self.capabilities.keys().copied().collect()
    }

    /// Add `other`'s denials; an entry already denied keeps its rule.
    pub fn merge(&mut self, other: &Denials) {
        for (capability, rule) in &other.capabilities {
            self.deny(*capability, rule.as_str());
        }
        for (column, rule) in &other.columns {
            self.deny_column(column.as_str(), rule.as_str());
        }
    }

    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty() && self.columns.is_empty()
    }
}

/// The refusal of a write whose capability `rule` denies on `table`.
pub fn forbidden<T>(capability: Capability, rule: &str, table: &str) -> Result<T> {
    Err(refusal(
        "write forbidden",
        [
            ("forbidden", table),
            ("rule", rule),
            ("capability", capability.as_str()),
        ],
    ))
}

/// The refusal of a write that sets `column`, which `rule` made read-only
/// on `table`.
pub fn forbidden_column<T>(column: &str, rule: &str, table: &str) -> Result<T> {
    Err(refusal(
        "column is read-only",
        [("forbidden", table), ("rule", rule), ("column", column)],
    ))
}

/// Context values go in as plain text, not `Debug`-quoted as `error!` would
/// write them. `forbidden: <table>` then `rule: <rule>` lead, the form hosts
/// already read back from an access refusal.
fn refusal(message: &str, context: [(&str, &str); 3]) -> VantageError {
    let mut e = error!(message).mark_forbidden();
    for (key, value) in context {
        e.context.insert(key.to_string(), value.to_string());
    }
    e
}
