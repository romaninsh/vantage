//! Comparison operator for a filter condition at the `Vista` boundary.
//!
//! Equality has always had a dedicated channel
//! ([`add_eq_condition`](crate::source::TableShell::add_eq_condition)); this
//! enum carries the rest so a caller can request `field != value`,
//! `field > value`, etc. through the general
//! [`add_op_condition`](crate::source::TableShell::add_op_condition) method.
//! Backends that can push the operator into their query advertise
//! [`can_filter_operators`](crate::VistaCapabilities::can_filter_operators);
//! others leave it `false` and the consumer filters locally.

use serde::{Deserialize, Serialize};

/// A comparison operator. The scalar operators mirror the query-builder
/// vocabulary the SQL and SurrealDB expression layers already expose
/// (`eq`/`ne`/`gt`/…); [`InSet`](FilterOp::InSet) / [`NotInSet`](FilterOp::NotInSet)
/// test membership against a list and carry a [`ciborium::Value::Array`] operand
/// rather than a scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    /// `field == value`
    Eq,
    /// `field != value`
    Ne,
    /// `field > value`
    Gt,
    /// `field >= value`
    Gte,
    /// `field < value`
    Lt,
    /// `field <= value`
    Lte,
    /// `field ∈ value` — `value` is an array; matches when the cell equals any
    /// element.
    InSet,
    /// `field ∉ value` — `value` is an array; matches when the cell equals no
    /// element.
    NotInSet,
    /// `field ~ value` — a pattern match on text. What "pattern" means is the
    /// backend's: SurrealDB's fuzzy `~`, SQL's `LIKE '%value%'`. A backend
    /// with no spelling for it returns Unimplemented and the consumer
    /// evaluates it locally as a case-insensitive substring test, which is
    /// the one reading every backend's version contains.
    Like,
}

impl FilterOp {
    /// The operator an author or a user wrote: a symbol (`>=`, `!=`, `~`) or
    /// a word (`gte`, `ne`, `like`). The symbol set is what a filter field
    /// accepts as a typed prefix, so it must stay short enough to type.
    pub fn parse(op: &str) -> Option<Self> {
        Some(match op.trim().to_ascii_lowercase().as_str() {
            "eq" | "=" | "==" => FilterOp::Eq,
            "ne" | "!=" | "<>" | "!" => FilterOp::Ne,
            "gt" | ">" => FilterOp::Gt,
            "gte" | ">=" => FilterOp::Gte,
            "lt" | "<" => FilterOp::Lt,
            "lte" | "<=" => FilterOp::Lte,
            "in" | "in_set" => FilterOp::InSet,
            "not_in" | "not_in_set" | "nin" | "!in" => FilterOp::NotInSet,
            "like" | "~" | "contains" => FilterOp::Like,
            _ => return None,
        })
    }

    /// Split a leading operator off a typed value: `">30"` → `(Gt, "30")`,
    /// `"~smith"` → `(Like, "smith")`, `"30"` → `(Eq, "30")`.
    pub fn split_prefix(text: &str) -> (Self, &str) {
        use FilterOp::*;
        let text = text.trim_start();
        // Two-character symbols first, or `>=` reads as `>` then `=30`.
        for (sym, op) in [
            (">=", Gte),
            ("<=", Lte),
            ("!=", Ne),
            ("<>", Ne),
            (">", Gt),
            ("<", Lt),
            ("~", Like),
            ("!", Ne),
            ("=", Eq),
        ] {
            if let Some(rest) = text.strip_prefix(sym) {
                return (op, rest.trim_start());
            }
        }
        (Eq, text)
    }

    /// How the operator reads in a chip or a label: nothing for equality —
    /// `Shop: Bristol` says it — and the symbol otherwise.
    pub fn display_symbol(&self) -> &'static str {
        match self {
            FilterOp::Eq => "",
            FilterOp::Ne => "≠",
            FilterOp::Gt => ">",
            FilterOp::Gte => "≥",
            FilterOp::Lt => "<",
            FilterOp::Lte => "≤",
            FilterOp::InSet => "in",
            FilterOp::NotInSet => "not in",
            FilterOp::Like => "~",
        }
    }

    /// Whether this operator's operand is a list (`InSet` / `NotInSet`) rather
    /// than a scalar. Callers building the CBOR operand use this to know
    /// whether to wrap the value in a [`ciborium::Value::Array`].
    pub fn takes_set(&self) -> bool {
        matches!(self, FilterOp::InSet | FilterOp::NotInSet)
    }

    /// Compact, stable symbol used when rendering the operator into a cache
    /// key (see [`Vista::index_key`](crate::Vista::index_key)). Distinct
    /// operators must produce distinct symbols so two filters that differ only
    /// by operator can't collide onto the same cached index.
    pub fn key_symbol(&self) -> &'static str {
        match self {
            FilterOp::Eq => "=",
            FilterOp::Ne => "!=",
            FilterOp::Gt => ">",
            FilterOp::Gte => ">=",
            FilterOp::Lt => "<",
            FilterOp::Lte => "<=",
            FilterOp::InSet => "in",
            FilterOp::NotInSet => "!in",
            FilterOp::Like => "~",
        }
    }

    /// Whether an `Ordering` of `value` relative to the operand satisfies this
    /// operator. `Eq`/`Ne` are handled by the caller (they don't need a total
    /// order); this covers the four ordered comparisons. `ordering` is
    /// `cmp(cell, operand)`.
    pub fn matches_ordering(&self, ordering: std::cmp::Ordering) -> bool {
        use std::cmp::Ordering::{Equal, Greater, Less};
        match self {
            FilterOp::Gt => ordering == Greater,
            FilterOp::Gte => ordering == Greater || ordering == Equal,
            FilterOp::Lt => ordering == Less,
            FilterOp::Lte => ordering == Less || ordering == Equal,
            // Eq/Ne and the set operators don't use a total order; treat as
            // non-matching here so a misuse fails closed rather than silently
            // passing everything. Callers handle these operators directly.
            FilterOp::Eq | FilterOp::Ne | FilterOp::InSet | FilterOp::NotInSet | FilterOp::Like => {
                false
            }
        }
    }
}
