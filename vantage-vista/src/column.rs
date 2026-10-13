use std::sync::Arc;

use ciborium::Value as CborValue;
use serde::{Deserialize, Serialize};
use vantage_core::Result;
use vantage_types::Record;

mod flag;
pub use flag::ColumnFlag;

/// Computes one column value from a record: the record as read so far goes
/// in, the column's value comes out.
pub(crate) type LazyValueFn = Arc<dyn Fn(&Record<CborValue>) -> Result<CborValue> + Send + Sync>;

/// Display-relevant column metadata held by a `Vista`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Column {
    pub name: String,
    /// The backend's or the spec's name for the column's type; `None` when
    /// nothing declares one (a YAML column without `type:`, a schemaless
    /// store).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    flags: Vec<ColumnFlag>,
    /// Set on a computed column; see `Column::with_expression` (`rhai` feature).
    #[serde(skip)]
    computed: Option<Computed>,
}

/// A computed column's script, kept for introspection, and its compiled form.
#[derive(Clone)]
struct Computed {
    code: String,
    eval: LazyValueFn,
}

impl std::fmt::Debug for Computed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Computed").field(&self.code).finish()
    }
}

impl Column {
    /// A column whose type is `original_type`.
    pub fn new(name: impl Into<String>, original_type: impl Into<String>) -> Self {
        Self::with_type(name.into(), Some(original_type.into()))
    }

    /// A column with no declared type.
    pub fn untyped(name: impl Into<String>) -> Self {
        Self::with_type(name.into(), None)
    }

    fn with_type(name: String, original_type: Option<String>) -> Self {
        Self {
            name,
            original_type,
            flags: Vec::new(),
            computed: None,
        }
    }

    /// Make this a computed column: `code` is a Rhai expression over `row`,
    /// the record as read so far, and its value becomes this column's value.
    ///
    /// ```rhai
    /// row.contents.split("\n").len() - 1
    /// ```
    ///
    /// The `Vista` fills computed columns on every read, in declaration
    /// order, so one sees the values of those declared before it. The backend
    /// never sees them: writes drop them, and filtering or ordering on one is
    /// an error. The column is flagged [`Calculated`](ColumnFlag::Calculated)
    /// and loses [`Orderable`](ColumnFlag::Orderable) and
    /// [`Searchable`](ColumnFlag::Searchable).
    ///
    /// The script compiles here, so one that doesn't parse fails the build
    /// rather than the first read.
    #[cfg(feature = "rhai")]
    pub fn with_expression(mut self, code: impl Into<String>) -> Result<Self> {
        use vantage_core::{Context, error};

        let code = code.into();
        let eval = crate::rhai::lazy_value_closure(&code).with_context(|| {
            error!(
                "Computed column expression doesn't compile",
                column = self.name.clone()
            )
        })?;
        self.flags
            .retain(|f| *f != ColumnFlag::Orderable && *f != ColumnFlag::Searchable);
        self = self.with_flag(ColumnFlag::Calculated);
        self.computed = Some(Computed { code, eval });
        Ok(self)
    }

    /// This column with `flag` added; a flag it already has is kept once.
    /// A flag is stored in its canonical form (`Other("id")` as `Id`).
    pub fn with_flag(mut self, flag: impl Into<ColumnFlag>) -> Self {
        let flag = ColumnFlag::from(flag.into().as_str());
        if !self.flags.contains(&flag) {
            self.flags.push(flag);
        }
        self
    }

    pub fn has_flag(&self, flag: impl Into<ColumnFlag>) -> bool {
        self.flags.contains(&flag.into())
    }

    pub fn flags(&self) -> &[ColumnFlag] {
        &self.flags
    }

    /// Whether the `Vista` computes this column rather than reading it.
    ///
    /// The expression isn't serialized: a column that goes through serde
    /// comes back stored, keeping only its [`Calculated`](ColumnFlag::Calculated)
    /// flag.
    pub fn is_computed(&self) -> bool {
        self.computed.is_some()
    }

    /// The script of a computed column.
    pub fn expression(&self) -> Option<&str> {
        self.computed.as_ref().map(|c| c.code.as_str())
    }

    /// This computed column's value for `row`; `None` for a stored column.
    pub(crate) fn compute(&self, row: &Record<CborValue>) -> Option<Result<CborValue>> {
        self.computed.as_ref().map(|c| (c.eval)(row))
    }
}

#[cfg(test)]
mod tests {
    use super::Column;

    #[test]
    fn an_untyped_column_serializes_without_a_type() {
        let json = serde_json::to_value(Column::untyped("note")).unwrap();
        assert_eq!(json, serde_json::json!({ "name": "note" }));
        let back: Column = serde_json::from_value(json).unwrap();
        assert_eq!(back.original_type, None);

        let typed = serde_json::to_value(Column::new("n", "int")).unwrap();
        assert_eq!(typed["original_type"], "int");
    }
}
