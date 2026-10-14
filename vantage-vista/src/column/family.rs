//! What kind of value a declared column type names.
//!
//! Declared types come from many places: a YAML spec (`i64`, `varchar(255)`),
//! a SQL catalog (`bigint`, `numeric(10, 2)`), or a table's Rust type through
//! `std::any::type_name` (`alloc::string::String`, `core::option::Option<i64>`,
//! `rust_decimal::decimal::Decimal`). Each is reduced to one base name before
//! it is classified, so every spelling of a type lands in the same family.

use super::Column;

/// The family of a column's declared type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColumnFamily {
    Integer,
    Float,
    /// Exact decimals (`decimal`, `numeric`): numeric, but never parsed
    /// through a float.
    Decimal,
    Text,
    Bool,
}

impl Column {
    /// The family of the declared type; `None` when the column is untyped or
    /// its type names none of the families.
    pub fn family(&self) -> Option<ColumnFamily> {
        let name = base_name(self.original_type.as_deref()?);
        let family = match name.as_str() {
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
            | "u128" | "usize" | "int" | "integer" | "bigint" | "smallint" | "serial"
            | "bigserial" => ColumnFamily::Integer,
            "f32" | "f64" | "float" | "double" | "real" | "number" => ColumnFamily::Float,
            "decimal" | "numeric" => ColumnFamily::Decimal,
            "string" | "str" | "text" | "varchar" | "char" => ColumnFamily::Text,
            "bool" | "boolean" => ColumnFamily::Bool,
            _ => return None,
        };
        Some(family)
    }
}

/// `t` reduced to its lowercase base name: `Option<..>` unwrapped, a Rust
/// path cut to its last segment, a reference's `&` and SQL's `(..)`
/// parameters dropped.
fn base_name(t: &str) -> String {
    let t = t.trim().trim_start_matches('&').trim();
    let (head, args) = match (t.find('<'), t.rfind('>')) {
        (Some(open), Some(close)) if open < close => (&t[..open], Some(&t[open + 1..close])),
        _ => (t, None),
    };
    let name = head.rsplit("::").next().unwrap_or(head);
    if let Some(inner) = args
        && name == "Option"
    {
        return base_name(inner);
    }
    let name = name.split('(').next().unwrap_or(name);
    name.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::any::type_name;

    use super::*;

    fn family(t: &str) -> Option<ColumnFamily> {
        Column::new("c", t).family()
    }

    #[test]
    fn rust_type_names_classify_like_their_short_names() {
        assert_eq!(family(type_name::<String>()), Some(ColumnFamily::Text));
        assert_eq!(family(type_name::<&str>()), Some(ColumnFamily::Text));
        assert_eq!(family(type_name::<Option<i64>>()), Some(ColumnFamily::Integer));
        assert_eq!(family(type_name::<Option<String>>()), Some(ColumnFamily::Text));
        assert_eq!(family(type_name::<bool>()), Some(ColumnFamily::Bool));
        assert_eq!(family(type_name::<f64>()), Some(ColumnFamily::Float));
        assert_eq!(
            family("rust_decimal::decimal::Decimal"),
            Some(ColumnFamily::Decimal)
        );
    }

    #[test]
    fn spec_and_sql_type_names_classify_case_insensitively() {
        assert_eq!(family("BIGINT"), Some(ColumnFamily::Integer));
        assert_eq!(family("varchar(255)"), Some(ColumnFamily::Text));
        assert_eq!(family("numeric(10, 2)"), Some(ColumnFamily::Decimal));
        assert_eq!(family("Boolean"), Some(ColumnFamily::Bool));
        assert_eq!(family("jsonb"), None);
        assert_eq!(family(type_name::<Vec<u8>>()), None);
        assert_eq!(Column::untyped("c").family(), None);
    }
}
