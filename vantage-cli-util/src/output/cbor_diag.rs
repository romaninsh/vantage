//! CBOR Diagnostic Notation writer (RFC 8949 §8).
//!
//! Lossless, human-readable, deterministic — the format used for golden
//! test fixtures. Every Vista driver must produce byte-identical output
//! for the same logical data when the CLI is run with `--format=cbor-diag`.
//!
//! Encoding rules used here:
//! - Integers print as decimal (`42`, `-7`).
//! - Floats always include a decimal point (`42.0`, never `42`). NaN/Inf
//!   render as `NaN`, `Infinity`, `-Infinity`.
//! - Text strings double-quoted, with `\\`, `\"`, `\n`, `\r`, `\t`
//!   escaped; other control chars as `\u{xxxx}`.
//! - Byte strings as `h'<hex>'`.
//! - Arrays `[a, b, c]`. Maps `{k: v, k: v}` with keys rendered as
//!   diagnostic values (strings stay quoted).
//! - `null`, `true`, `false`, `undefined` as literals.
//! - Tagged values as `N(inner)`.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_types::Record;

/// Render a list of records keyed by id. Each record becomes a map; the
/// outer structure is itself a map from id to record. Diagnostic notation
/// has no concept of "records" so the framing is the natural CBOR map.
pub fn write_list(records: &IndexMap<String, Record<CborValue>>) -> String {
    let mut out = String::from("{");
    let mut first = true;
    for (id, record) in records {
        if !first {
            out.push_str(", ");
        }
        first = false;
        out.push_str(&write_text(id));
        out.push_str(": ");
        out.push_str(&write_record_map(record));
    }
    out.push('}');
    out.push('\n');
    out
}

pub fn write_record(id: &str, record: &Record<CborValue>) -> String {
    let mut out = String::new();
    out.push_str(&write_text(id));
    out.push_str(": ");
    out.push_str(&write_record_map(record));
    out.push('\n');
    out
}

pub fn write_scalar(label: &str, value: &CborValue) -> String {
    let mut out = String::new();
    out.push_str(&write_text(label));
    out.push_str(": ");
    out.push_str(&write_value(value));
    out.push('\n');
    out
}

fn write_record_map(record: &Record<CborValue>) -> String {
    let mut out = String::from("{");
    let mut first = true;
    for (key, value) in record.iter() {
        if !first {
            out.push_str(", ");
        }
        first = false;
        out.push_str(&write_text(key));
        out.push_str(": ");
        out.push_str(&write_value(value));
    }
    out.push('}');
    out
}

/// Public so the scalar/list/record callers and tests share one impl.
pub fn write_value(v: &CborValue) -> String {
    match v {
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Float(f) => write_float(*f),
        CborValue::Text(s) => write_text(s),
        CborValue::Bytes(b) => write_bytes(b),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Null => "null".to_string(),
        CborValue::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&write_value(item));
            }
            out.push(']');
            out
        }
        CborValue::Map(pairs) => {
            let mut out = String::from("{");
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&write_value(k));
                out.push_str(": ");
                out.push_str(&write_value(v));
            }
            out.push('}');
            out
        }
        CborValue::Tag(tag, inner) => format!("{tag}({})", write_value(inner)),
        // ciborium::Value is non-exhaustive — keep a catch-all that
        // preserves the principle "any CBOR value renders to *something*".
        other => format!("{other:?}"),
    }
}

fn write_float(f: f64) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        };
    }
    let s = format!("{f}");
    // Force a decimal point so `42_f64` doesn't render the same as the
    // integer `42` — the whole point of cbor-diag is type preservation.
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

fn write_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{{{:04x}}}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn write_bytes(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len() * 2 + 3);
    out.push_str("h'");
    for byte in b {
        out.push_str(&format!("{byte:02x}"));
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars() {
        assert_eq!(write_value(&CborValue::Integer(42.into())), "42");
        assert_eq!(write_value(&CborValue::Integer((-7).into())), "-7");
        assert_eq!(write_value(&CborValue::Float(2.5)), "2.5");
        assert_eq!(write_value(&CborValue::Float(42.0)), "42.0");
        assert_eq!(write_value(&CborValue::Bool(true)), "true");
        assert_eq!(write_value(&CborValue::Bool(false)), "false");
        assert_eq!(write_value(&CborValue::Null), "null");
    }

    #[test]
    fn text_escapes() {
        assert_eq!(write_value(&CborValue::Text("hi".into())), "\"hi\"");
        assert_eq!(write_value(&CborValue::Text("a\"b".into())), "\"a\\\"b\"");
        assert_eq!(
            write_value(&CborValue::Text("line\nbreak".into())),
            "\"line\\nbreak\""
        );
        assert_eq!(
            write_value(&CborValue::Text("\u{0001}".into())),
            "\"\\u{0001}\""
        );
    }

    #[test]
    fn bytes_as_hex() {
        assert_eq!(
            write_value(&CborValue::Bytes(vec![0xde, 0xad, 0xbe, 0xef])),
            "h'deadbeef'"
        );
        assert_eq!(write_value(&CborValue::Bytes(vec![])), "h''");
    }

    #[test]
    fn arrays_and_maps() {
        let arr = CborValue::Array(vec![
            CborValue::Integer(1.into()),
            CborValue::Text("two".into()),
            CborValue::Bool(true),
        ]);
        assert_eq!(write_value(&arr), "[1, \"two\", true]");

        let map = CborValue::Map(vec![
            (CborValue::Text("a".into()), CborValue::Integer(1.into())),
            (CborValue::Text("b".into()), CborValue::Null),
        ]);
        assert_eq!(write_value(&map), "{\"a\": 1, \"b\": null}");
    }

    #[test]
    fn tagged() {
        let t = CborValue::Tag(0, Box::new(CborValue::Text("2024-01-01".into())));
        assert_eq!(write_value(&t), "0(\"2024-01-01\")");
    }

    #[test]
    fn float_specials() {
        assert_eq!(write_float(f64::NAN), "NaN");
        assert_eq!(write_float(f64::INFINITY), "Infinity");
        assert_eq!(write_float(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(write_float(42.0), "42.0");
        assert_eq!(write_float(1e10), "10000000000.0");
    }

    #[test]
    fn list_framing() {
        let mut records: IndexMap<String, Record<CborValue>> = IndexMap::new();
        let mut r1 = Record::new();
        r1.insert("name".to_string(), CborValue::Text("alice".into()));
        records.insert("u1".to_string(), r1);
        let mut r2 = Record::new();
        r2.insert("name".to_string(), CborValue::Text("bob".into()));
        records.insert("u2".to_string(), r2);

        let s = write_list(&records);
        assert_eq!(
            s,
            "{\"u1\": {\"name\": \"alice\"}, \"u2\": {\"name\": \"bob\"}}\n"
        );
    }

    #[test]
    fn scalar_framing() {
        assert_eq!(
            write_scalar("sum(price)", &CborValue::Integer(42.into())),
            "\"sum(price)\": 42\n"
        );
    }
}
