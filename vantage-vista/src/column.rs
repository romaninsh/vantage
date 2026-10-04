use std::sync::Arc;

use ciborium::Value as CborValue;
use serde::{Deserialize, Serialize};
use vantage_core::Result;
use vantage_types::Record;

use crate::flags;

/// Computes one column value from a record: the record as read so far goes
/// in, the column's value comes out.
pub type LazyValueFn = Arc<dyn Fn(&Record<CborValue>) -> Result<CborValue> + Send + Sync>;

/// Display-relevant column metadata held by a `Vista`.
///
/// `flags` is an open vocabulary; the constants in [`crate::flags`] name the
/// values vista's own accessors understand. Drivers and consumers may add
/// their own.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Column {
    pub name: String,
    pub original_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
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
    pub fn new(name: impl Into<String>, original_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            original_type: original_type.into(),
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
    /// an error. The column is flagged [`CALCULATED`](flags::CALCULATED) and
    /// loses [`ORDERABLE`](flags::ORDERABLE) and
    /// [`SEARCHABLE`](flags::SEARCHABLE).
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
            .retain(|f| f != flags::ORDERABLE && f != flags::SEARCHABLE);
        if !self.has_flag(flags::CALCULATED) {
            self.flags.push(flags::CALCULATED.to_string());
        }
        self.computed = Some(Computed { code, eval });
        Ok(self)
    }

    pub fn with_flag(mut self, flag: impl Into<String>) -> Self {
        self.flags.push(flag.into());
        self
    }

    pub fn hidden(self) -> Self {
        self.with_flag(flags::HIDDEN)
    }

    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    pub fn is_hidden(&self) -> bool {
        self.has_flag(flags::HIDDEN)
    }

    pub fn is_id(&self) -> bool {
        self.has_flag(flags::ID)
    }

    pub fn is_title(&self) -> bool {
        self.has_flag(flags::TITLE)
    }

    /// Whether the `Vista` computes this column rather than reading it.
    ///
    /// The expression isn't serialized: a column that goes through serde
    /// comes back stored, keeping only its [`CALCULATED`](flags::CALCULATED)
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
