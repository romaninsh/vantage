//! Converting a configured operand to the type its column declares.
//!
//! Configuration carries operands without knowing the backend's types: a
//! YAML `code: "200"` may meet an integer column, and `code: 200` a text
//! one. The source schema says which, so the operand is converted to the
//! declared type's family ([`Column::family`]) before it is compared. Without
//! a declared type there is nothing to convert to, and the operand compares
//! as given.
//!
//! Exact decimals are never converted: parsing `"0.1"` through a float would
//! compare a different number. A tagged cell (a decimal, a uuid, a record id)
//! is not matched against a text operand either.

use ciborium::Value as CborValue;
use ciborium::value::Integer;
use vantage_vista::{Column, ColumnFamily};

use crate::aggregate::cmp;

/// `value` converted to the family `column` declares: text parsed as an
/// integer (or, failing that, a float), a float or a bool; a scalar rendered
/// as text. An untyped column, a decimal or unknown family, or an operand that
/// doesn't convert, stays as given.
pub(crate) fn coerce_operand(value: &CborValue, column: Option<&Column>) -> CborValue {
    let Some((column, family)) = column.and_then(|c| Some((c, c.family()?))) else {
        return value.clone();
    };
    let converted = match (family, value) {
        (ColumnFamily::Integer, CborValue::Text(text)) => {
            parse_integer(text).or_else(|| parse_float(text))
        }
        (ColumnFamily::Float, CborValue::Text(text)) => parse_float(text),
        (ColumnFamily::Bool, CborValue::Text(text)) => parse_bool(text),
        (ColumnFamily::Text, CborValue::Text(_)) => return value.clone(),
        (ColumnFamily::Text, scalar) => cmp::scalar_text(scalar).map(CborValue::Text),
        _ => return value.clone(),
    };
    converted.unwrap_or_else(|| {
        tracing::debug!(
            column = %column.name,
            r#type = column.original_type.as_deref().unwrap_or_default(),
            operand = ?value,
            "operand doesn't convert to the column's type; compared as given"
        );
        value.clone()
    })
}

fn parse_integer(text: &str) -> Option<CborValue> {
    let i = text.trim().parse::<i128>().ok()?;
    Integer::try_from(i).ok().map(CborValue::Integer)
}

fn parse_float(text: &str) -> Option<CborValue> {
    text.trim().parse::<f64>().ok().map(CborValue::Float)
}

fn parse_bool(text: &str) -> Option<CborValue> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("true") {
        Some(CborValue::Bool(true))
    } else if text.eq_ignore_ascii_case("false") {
        Some(CborValue::Bool(false))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::any::type_name;

    use super::*;

    fn text(s: &str) -> CborValue {
        CborValue::Text(s.to_string())
    }

    fn coerced(value: CborValue, original_type: &str) -> CborValue {
        coerce_operand(&value, Some(&Column::new("c", original_type)))
    }

    #[test]
    fn an_operand_takes_its_column_family() {
        assert_eq!(
            coerced(text("200"), "BIGINT"),
            CborValue::Integer(200.into())
        );
        assert_eq!(coerced(text("3.0"), "i64"), CborValue::Float(3.0));
        assert_eq!(coerced(text("2.5"), "f64"), CborValue::Float(2.5));
        assert_eq!(coerced(text("TRUE"), "boolean"), CborValue::Bool(true));
        assert_eq!(
            coerced(CborValue::Integer(7.into()), "varchar(8)"),
            text("7")
        );
    }

    /// Table-backed vistas declare `std::any::type_name` strings.
    #[test]
    fn an_operand_takes_a_rust_type_names_family() {
        assert_eq!(
            coerced(CborValue::Integer(200.into()), type_name::<String>()),
            text("200")
        );
        assert_eq!(
            coerced(text("200"), type_name::<Option<i64>>()),
            CborValue::Integer(200.into())
        );
        assert_eq!(
            coerced(text("false"), type_name::<bool>()),
            CborValue::Bool(false)
        );
    }

    #[test]
    fn an_operand_that_doesnt_convert_stays_as_given() {
        assert_eq!(coerced(text("n/a"), "i64"), text("n/a"));
        assert_eq!(coerced(text("yes"), "bool"), text("yes"));
        assert_eq!(coerced(text("0.1"), "numeric(10, 2)"), text("0.1"));
        assert_eq!(coerced(text("200"), "jsonb"), text("200"));
        assert_eq!(
            coerce_operand(&text("200"), Some(&Column::untyped("c"))),
            text("200")
        );
        assert_eq!(coerce_operand(&text("200"), None), text("200"));
    }
}
