//! What a shell can do: [`Capability`], the [`VistaCapabilities`] set a shell
//! advertises, and [`unsupported`], the refusal a shell answers with.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use vantage_core::{Result, VantageError, error};

mod set;
#[cfg(test)]
mod tests;

pub use set::VistaCapabilities;

/// One method family of [`TableShell`](crate::TableShell) that a shell may or
/// may not implement.
///
/// Each capability names exactly one family, and a shell advertising it must
/// answer that family without refusing (the `contract` feature checks this):
///
/// | Capability | Methods |
/// |---|---|
/// | `Insert` | `insert` |
/// | `Update` | `replace`, `upsert`, `patch` |
/// | `Delete` | `delete`, `delete_all` |
/// | `Import` | `import`: one atomic batch that counts only new rows |
/// | `Subscribe` | `watch` |
/// | `Order` | `set_order` |
/// | `Search` | `set_search` |
/// | `FilterOps` | `add_condition` with any operator other than `Eq` |
/// | `NativeConditions` | `add_native_condition` |
/// | `Window` | `fetch_window` at any offset |
/// | `Cursor` | `fetch_cursor` |
/// | `Traverse` | `get_ref` |
/// | `ConfineWrites` | every write keeps to the narrowed set (see `WritableValueSet`) |
///
/// `list`, `get`, `count` and equality conditions have no capability: every
/// shell answers them. Nor does the leading window, `fetch_window(0, n)`,
/// which every shell that lists answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Capability {
    Insert,
    Update,
    Delete,
    Import,
    Subscribe,
    Order,
    Search,
    FilterOps,
    NativeConditions,
    Window,
    Cursor,
    Traverse,
    ConfineWrites,
}

/// Every variant, in declaration order. The set is a `u16` bitset, so this
/// holds at most 16.
const ALL: [Capability; 13] = [
    Capability::Insert,
    Capability::Update,
    Capability::Delete,
    Capability::Import,
    Capability::Subscribe,
    Capability::Order,
    Capability::Search,
    Capability::FilterOps,
    Capability::NativeConditions,
    Capability::Window,
    Capability::Cursor,
    Capability::Traverse,
    Capability::ConfineWrites,
];

impl Capability {
    /// Every capability, in declaration order. Serde, the rhai
    /// `capabilities()` map and the contract checks all iterate this.
    pub fn iter() -> impl Iterator<Item = Capability> {
        ALL.into_iter()
    }

    /// The snake_case name used in serde, scripts and error context.
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::Insert => "insert",
            Capability::Update => "update",
            Capability::Delete => "delete",
            Capability::Import => "import",
            Capability::Subscribe => "subscribe",
            Capability::Order => "order",
            Capability::Search => "search",
            Capability::FilterOps => "filter_ops",
            Capability::NativeConditions => "native_conditions",
            Capability::Window => "window",
            Capability::Cursor => "cursor",
            Capability::Traverse => "traverse",
            Capability::ConfineWrites => "confine_writes",
        }
    }

    /// The capability named `name` (its [`as_str`](Self::as_str) form).
    pub(crate) fn parse(name: &str) -> Option<Capability> {
        Capability::iter().find(|c| c.as_str() == name)
    }

    fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Capability {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Capability::parse(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown capability `{name}`")))
    }
}

/// The refusal of a method whose capability the shell doesn't have: an
/// `Unsupported` error naming the capability. Default trait methods answer
/// with it; a caller probing a capability reads it as "no", not as a fault.
pub fn unsupported<T>(capability: Capability) -> Result<T> {
    Err(error!(
        "capability is not supported; method refused",
        capability = capability.as_str()
    )
    .mark_unsupported()
    .traced_debug())
}

/// The error for a relation a shell doesn't declare, marked
/// [`not_found`](vantage_core::VantageError::is_not_found) so a caller can
/// tell a missing relation from one that fails to traverse.
pub fn unknown_relation(relation: &str) -> VantageError {
    error!("unknown relation", relation = relation).mark_not_found()
}
