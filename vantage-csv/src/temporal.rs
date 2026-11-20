//! Columns declared `date`, `time` or `timestamp`. CSV keeps them as text in any accepted form;
//! a read turns each cell into the carrier's text form, or fails naming the column and the cell,
//! so rows compare and filter by value. The vista tags that text as the carrier value.

#[cfg(feature = "vista")]
use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::temporal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Temporal {
    Date,
    Time,
    Instant,
}

impl Temporal {
    /// The kind a typed column holds, from its Rust type name: `NaiveDate`, `NaiveTime`, or a
    /// `DateTime` (naive text is taken as UTC).
    pub(crate) fn of_type(type_name: &str) -> Option<Self> {
        if type_name.contains("DateTime") {
            Some(Self::Instant)
        } else if type_name.contains("NaiveDate") {
            Some(Self::Date)
        } else if type_name.contains("NaiveTime") {
            Some(Self::Time)
        } else {
            None
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Date => "date",
            Self::Time => "time",
            Self::Instant => "timestamp",
        }
    }

    /// A cell's text in the carrier's text form.
    pub(crate) fn cell_text(self, column: &str, raw: &str) -> Result<String> {
        let text = match self {
            Self::Date => temporal::parse_date(raw).map(|d| temporal::format_date(&d)),
            Self::Time => temporal::parse_time(raw).map(|t| temporal::format_time(&t)),
            Self::Instant => temporal::parse_instant(raw).map(|at| temporal::format_instant(&at)),
        };
        text.ok_or_else(|| {
            error!(
                format!("CSV cell is not a {}", self.name()),
                column = column,
                value = raw
            )
        })
    }

    /// A condition value, in any accepted carrier form, in the carrier's text form.
    #[cfg(feature = "vista")]
    pub(crate) fn value_text(self, column: &str, value: &CborValue) -> Result<String> {
        let text = match self {
            Self::Date => temporal::as_date(value).map(|d| temporal::format_date(&d)),
            Self::Time => temporal::as_time(value).map(|t| temporal::format_time(&t)),
            Self::Instant => temporal::as_instant(value).map(|at| temporal::format_instant(&at)),
        };
        text.ok_or_else(|| {
            error!(
                format!("Condition value is not a {}", self.name()),
                column = column,
                value = value
            )
        })
    }

    /// Canonical text as the carrier value.
    #[cfg(feature = "vista")]
    pub(crate) fn carrier(self, text: String) -> CborValue {
        let tag = match self {
            Self::Date => temporal::TAG_DATE,
            Self::Time => temporal::TAG_TIME,
            Self::Instant => temporal::TAG_INSTANT,
        };
        CborValue::Tag(tag, Box::new(CborValue::Text(text)))
    }
}
