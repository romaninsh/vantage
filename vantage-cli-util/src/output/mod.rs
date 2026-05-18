//! Output formatters for CLI results.
//!
//! The CLI doubles as a test-scenario runner across persistences, so output
//! must be both human-readable (default `table`, ANSI styled via the
//! crate-internal `table_display` module) and machine-comparable. JSON
//! variants are the ergonomic-but-lossy options; `cbor-diag` (RFC 8949 §8
//! diagnostic notation) is the lossless form used for golden test fixtures.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_types::Record;

pub mod cbor_diag;
pub mod json;
pub mod ndjson;

/// Output format selector passed into renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Ndjson,
    CborDiag,
}

impl OutputFormat {
    /// Parse the value of a `--format=…` flag. Returns `None` on unknown
    /// names so call sites can produce their own error text.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "table" => Some(Self::Table),
            "json" => Some(Self::Json),
            "ndjson" => Some(Self::Ndjson),
            "cbor-diag" => Some(Self::CborDiag),
            _ => None,
        }
    }
}

/// Render a list of records to a String in the chosen non-table format.
///
/// `Table` is handled by `table_display` directly (it needs ANSI/terminal
/// concerns) and is not produced here — calling `render_list` with
/// `OutputFormat::Table` is a programming error and returns an empty
/// string.
pub fn render_list(format: OutputFormat, records: &IndexMap<String, Record<CborValue>>) -> String {
    match format {
        OutputFormat::Table => String::new(),
        OutputFormat::Json => json::write_list(records),
        OutputFormat::Ndjson => ndjson::write_list(records),
        OutputFormat::CborDiag => cbor_diag::write_list(records),
    }
}

/// Render a single record to a String in the chosen non-table format.
pub fn render_record(format: OutputFormat, id: &str, record: &Record<CborValue>) -> String {
    match format {
        OutputFormat::Table => String::new(),
        OutputFormat::Json => json::write_record(id, record),
        OutputFormat::Ndjson => ndjson::write_record(id, record),
        OutputFormat::CborDiag => cbor_diag::write_record(id, record),
    }
}

/// Render a scalar (aggregate result) to a String.
pub fn render_scalar(format: OutputFormat, label: &str, value: &CborValue) -> String {
    match format {
        OutputFormat::Table => String::new(),
        OutputFormat::Json => json::write_scalar(label, value),
        OutputFormat::Ndjson => ndjson::write_scalar(label, value),
        OutputFormat::CborDiag => cbor_diag::write_scalar(label, value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_formats() {
        assert_eq!(OutputFormat::parse("table"), Some(OutputFormat::Table));
        assert_eq!(OutputFormat::parse("json"), Some(OutputFormat::Json));
        assert_eq!(OutputFormat::parse("ndjson"), Some(OutputFormat::Ndjson));
        assert_eq!(
            OutputFormat::parse("cbor-diag"),
            Some(OutputFormat::CborDiag)
        );
        assert_eq!(OutputFormat::parse("yaml"), None);
    }
}
