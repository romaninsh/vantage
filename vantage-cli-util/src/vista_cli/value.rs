//! Value coercion for CLI condition arguments.
//!
//! Three paths:
//! - **Column-typed** ([`coerce_for_column`]): the target column's
//!   declared type wins. `name=true` against a string column stays a
//!   string; `is_paying_client=true` against a bool column becomes a
//!   bool. Used by the Eq-condition wiring in `run` where the field
//!   name is known.
//! - **Auto-detect** ([`auto_detect`]): try integer → float → bool →
//!   text. The fallback when no column type is available (unknown
//!   columns, value-list elements for `:in=…`, narrowing on
//!   driver-returned id strings).
//! - **Typed JSON** (`#`-prefixed): forces the value to be parsed as a
//!   JSON literal, which preserves bool/int/float/null/array/map types
//!   unambiguously. Necessary when a string happens to look like a
//!   number or boolean, or when a heterogeneous list / nested map is
//!   needed.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_vista::Vista;

/// Coerce a raw CLI value string to CBOR. `#`-prefixed values are parsed
/// as JSON literals; everything else goes through the auto-detect path.
pub fn parse_value(s: &str) -> Result<CborValue> {
    if let Some(rest) = s.strip_prefix('#') {
        let json: serde_json::Value = serde_json::from_str(rest)
            .map_err(|e| error!(format!("invalid JSON literal `{rest}`: {e}")))?;
        Ok(json_to_cbor(json))
    } else {
        Ok(auto_detect(s))
    }
}

/// Column-typed coercion. Looks up `field` in `vista`'s metadata and
/// targets the matching CBOR variant — so `name=true` against a string
/// column produces `Text("true")`, not `Bool(true)`. Callers pass the
/// raw user-typed text (without any `#`-prefix; the `#`-typed escape
/// hatch is handled at the parser layer and stored as a pre-parsed
/// `CborValue` instead).
///
/// Returns an error when the raw value can't be parsed as the column's
/// declared type — e.g. `calories=abc` on an `i64` column. The user
/// almost certainly made a typo; surfacing it at the CLI boundary is
/// kinder than silently shipping an unmatched filter to the driver.
///
/// Falls back to [`auto_detect`] (never errors) when the column is
/// missing from the metadata or has a type the cli runner doesn't
/// recognise — preserving the old behaviour for callers that haven't
/// taught their drivers to surface a column type yet.
pub fn coerce_for_column(vista: &Vista, field: &str, raw: &str) -> Result<CborValue> {
    let Some(column) = vista.get_column(field) else {
        return Ok(auto_detect(raw));
    };
    match column_kind(&column.original_type) {
        ColumnKind::Bool => match raw {
            "true" => Ok(CborValue::Bool(true)),
            "false" => Ok(CborValue::Bool(false)),
            other => Err(error!(format!(
                "`{field}` is a bool column; value `{other}` is neither `true` nor `false`"
            ))),
        },
        ColumnKind::Int => raw
            .parse::<i64>()
            .map(|i| CborValue::Integer(i.into()))
            .map_err(|_| {
                error!(format!(
                    "`{field}` is an integer column; value `{raw}` is not an integer"
                ))
            }),
        ColumnKind::Float => raw.parse::<f64>().map(CborValue::Float).map_err(|_| {
            error!(format!(
                "`{field}` is a float column; value `{raw}` is not a number"
            ))
        }),
        ColumnKind::Text => Ok(CborValue::Text(raw.to_string())),
        ColumnKind::Unknown => Ok(auto_detect(raw)),
    }
}

enum ColumnKind {
    Bool,
    Int,
    Float,
    Text,
    Unknown,
}

/// Bucket a Rust type-name string (from `std::any::type_name::<T>()` or
/// a YAML type alias) into a coercion kind. Strips the leading module
/// path so `alloc::string::String` and `String` both land as `Text`.
fn column_kind(original_type: &str) -> ColumnKind {
    let tail = original_type.rsplit("::").next().unwrap_or(original_type);
    match tail {
        "bool" | "boolean" => ColumnKind::Bool,
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" | "int" | "integer" => ColumnKind::Int,
        "f32" | "f64" | "float" | "double" => ColumnKind::Float,
        "String" | "str" | "string" | "text" => ColumnKind::Text,
        _ => ColumnKind::Unknown,
    }
}

/// Cheap heuristic typing for unprefixed values: integer if it parses,
/// then float, then `true`/`false`, else text. Drivers translate
/// further at their own boundary.
pub fn auto_detect(value: &str) -> CborValue {
    if let Ok(i) = value.parse::<i64>() {
        CborValue::Integer(i.into())
    } else if let Ok(f) = value.parse::<f64>() {
        CborValue::Float(f)
    } else if value == "true" {
        CborValue::Bool(true)
    } else if value == "false" {
        CborValue::Bool(false)
    } else {
        CborValue::Text(value.to_string())
    }
}

/// Translate a `serde_json::Value` into the matching `ciborium::Value`.
/// Lossless for the JSON type set (the JSON Number's int-vs-float
/// distinction maps onto CBOR's `Integer` vs `Float`).
pub fn json_to_cbor(j: serde_json::Value) -> CborValue {
    use serde_json::Value as J;
    match j {
        J::Null => CborValue::Null,
        J::Bool(b) => CborValue::Bool(b),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                CborValue::Integer(i.into())
            } else if let Some(u) = n.as_u64() {
                // u64 in (i64::MAX, u64::MAX]; `ciborium::value::Integer`
                // takes a `u64` directly.
                CborValue::Integer(u.into())
            } else if let Some(f) = n.as_f64() {
                CborValue::Float(f)
            } else {
                CborValue::Null
            }
        }
        J::String(s) => CborValue::Text(s),
        J::Array(arr) => CborValue::Array(arr.into_iter().map(json_to_cbor).collect()),
        J::Object(map) => CborValue::Map(
            map.into_iter()
                .map(|(k, v)| (CborValue::Text(k), json_to_cbor(v)))
                .collect(),
        ),
    }
}

/// Parse a comma-separated value list for `field:in=a,b,c`. Each
/// element goes through `parse_value` so `#`-typed elements work
/// (`#1,#2,#"three"` etc., though `#[1,2,"three"]` is usually cleaner).
pub fn parse_value_list(s: &str) -> Result<Vec<CborValue>> {
    if let Some(rest) = s.strip_prefix('#') {
        // The whole list is a JSON array.
        let json: serde_json::Value = serde_json::from_str(rest)
            .map_err(|e| error!(format!("invalid JSON array `{rest}`: {e}")))?;
        match json {
            serde_json::Value::Array(arr) => Ok(arr.into_iter().map(json_to_cbor).collect()),
            other => Err(error!(format!(
                "`:in=#…` expects a JSON array, got `{other}`"
            ))),
        }
    } else {
        Ok(s.split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(auto_detect)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autodetect_kinds() {
        assert!(matches!(auto_detect("42"), CborValue::Integer(_)));
        assert!(matches!(auto_detect("3.14"), CborValue::Float(_)));
        assert_eq!(auto_detect("true"), CborValue::Bool(true));
        assert_eq!(auto_detect("false"), CborValue::Bool(false));
        assert_eq!(auto_detect("alice"), CborValue::Text("alice".into()));
    }

    #[test]
    fn typed_json_overrides_autodetect() {
        // Without `#`, "42" is an int.
        assert!(matches!(parse_value("42").unwrap(), CborValue::Integer(_)));
        // With `#"42"`, it's a string.
        assert_eq!(
            parse_value("#\"42\"").unwrap(),
            CborValue::Text("42".into())
        );
    }

    #[test]
    fn typed_json_bool_int_null() {
        assert_eq!(parse_value("#true").unwrap(), CborValue::Bool(true));
        assert!(matches!(parse_value("#42").unwrap(), CborValue::Integer(_)));
        assert_eq!(parse_value("#null").unwrap(), CborValue::Null);
    }

    #[test]
    fn typed_json_array() {
        let v = parse_value("#[1, 2, \"three\"]").unwrap();
        match v {
            CborValue::Array(items) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[2], CborValue::Text("three".into()));
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn typed_json_object() {
        let v = parse_value("#{\"nested\": \"obj\"}").unwrap();
        match v {
            CborValue::Map(pairs) => {
                assert_eq!(pairs.len(), 1);
                assert_eq!(pairs[0].0, CborValue::Text("nested".into()));
                assert_eq!(pairs[0].1, CborValue::Text("obj".into()));
            }
            other => panic!("expected Map, got {other:?}"),
        }
    }

    #[test]
    fn typed_json_bad_input() {
        assert!(parse_value("#{bad json").is_err());
    }

    #[test]
    fn value_list_split() {
        let xs = parse_value_list("a,b,c").unwrap();
        assert_eq!(xs.len(), 3);
        assert_eq!(xs[0], CborValue::Text("a".into()));

        let nums = parse_value_list("1,2,3").unwrap();
        assert!(matches!(nums[0], CborValue::Integer(_)));
    }

    #[test]
    fn value_list_json_array() {
        let xs = parse_value_list("#[1, \"two\", true]").unwrap();
        assert_eq!(xs.len(), 3);
        assert_eq!(xs[2], CborValue::Bool(true));
    }

    #[test]
    fn column_kind_handles_module_paths() {
        // Rust's `type_name::<String>()` is `alloc::string::String`.
        assert!(matches!(
            column_kind("alloc::string::String"),
            ColumnKind::Text
        ));
        // Bare aliases from YAML factories also classify correctly.
        assert!(matches!(column_kind("bool"), ColumnKind::Bool));
        assert!(matches!(column_kind("i64"), ColumnKind::Int));
        assert!(matches!(column_kind("f64"), ColumnKind::Float));
        assert!(matches!(column_kind("string"), ColumnKind::Text));
        assert!(matches!(column_kind("text"), ColumnKind::Text));
        assert!(matches!(column_kind("integer"), ColumnKind::Int));
        assert!(matches!(
            column_kind("custom::Wrapper"),
            ColumnKind::Unknown
        ));
    }

    #[test]
    fn coerce_for_column_targets_declared_type() {
        use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};
        let metadata = VistaMetadata::new()
            .with_column(Column::new("name", "String"))
            .with_column(Column::new("vip_flag", "bool"))
            .with_column(Column::new("salary", "i64"));
        let vista = Vista::new("users", Box::new(MockShell::new()), metadata);

        // `name=true` against a string column stays a string.
        assert_eq!(
            coerce_for_column(&vista, "name", "true").unwrap(),
            CborValue::Text("true".into())
        );
        // `name=42` against a string column stays a string.
        assert_eq!(
            coerce_for_column(&vista, "name", "42").unwrap(),
            CborValue::Text("42".into())
        );
        // `vip_flag=true` against a bool column becomes a bool.
        assert_eq!(
            coerce_for_column(&vista, "vip_flag", "true").unwrap(),
            CborValue::Bool(true)
        );
        // `salary=900` against an i64 column becomes an int.
        assert!(matches!(
            coerce_for_column(&vista, "salary", "900").unwrap(),
            CborValue::Integer(_)
        ));
        // Unknown column falls back to auto-detect (never errors).
        assert_eq!(
            coerce_for_column(&vista, "unseen", "true").unwrap(),
            CborValue::Bool(true)
        );
    }

    #[test]
    fn coerce_for_column_rejects_typo_against_typed_column() {
        use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};
        let metadata = VistaMetadata::new()
            .with_column(Column::new("vip_flag", "bool"))
            .with_column(Column::new("salary", "i64"))
            .with_column(Column::new("ratio", "f64"));
        let vista = Vista::new("users", Box::new(MockShell::new()), metadata);

        let bool_err = coerce_for_column(&vista, "vip_flag", "yes").unwrap_err();
        assert!(format!("{bool_err}").contains("bool column"));

        let int_err = coerce_for_column(&vista, "salary", "abc").unwrap_err();
        assert!(format!("{int_err}").contains("integer column"));

        let float_err = coerce_for_column(&vista, "ratio", "xyz").unwrap_err();
        assert!(format!("{float_err}").contains("float column"));
    }
}
