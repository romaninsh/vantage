//! CBOR Diagnostic Notation writer (RFC 8949 §8).
//!
//! Lossless, human-readable, deterministic — the format used for golden
//! test fixtures. Every Vista driver must produce byte-identical output
//! for the same logical data when the CLI is run with `--format=cbor-diag`.
//!
//! Encoding rules:
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

/// Render records as a map from id to record map.
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
        // `ciborium::Value` is non-exhaustive; any future variant still renders.
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
    // Keeps `42.0` distinct from the integer `42`.
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
