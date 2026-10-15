//! Lossy JSON output — best-effort, for piping to `jq` and human eyes.
//!
//! CBOR carries types JSON can't represent losslessly (int variants beyond
//! ±2^53, byte strings, tagged values). This writer makes a clean
//! best-effort:
//! - Integers print as JSON numbers when they fit in i64; otherwise as
//!   decimal strings.
//! - Floats print as JSON numbers (NaN/Inf → `null`, which is what
//!   `serde_json` does too).
//! - Byte strings render as `"0x<hex>"` — strings, not numbers, so they
//!   survive JSON-aware tools.
//! - Tagged values render their inner value; the tag is dropped. Use
//!   `cbor-diag` when the tag matters.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_types::Record;

pub fn write_list(records: &IndexMap<String, Record<CborValue>>) -> String {
    let mut out = String::from("{");
    let mut first = true;
    for (id, record) in records {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&write_string(id));
        out.push(':');
        out.push_str(&write_record_map(record));
    }
    out.push('}');
    out.push('\n');
    out
}

pub fn write_record(id: &str, record: &Record<CborValue>) -> String {
    let mut out = String::from("{");
    out.push_str(&write_string(id));
    out.push(':');
    out.push_str(&write_record_map(record));
    out.push('}');
    out.push('\n');
    out
}

pub fn write_scalar(label: &str, value: &CborValue) -> String {
    let mut out = String::from("{");
    out.push_str(&write_string(label));
    out.push(':');
    out.push_str(&write_value(value));
    out.push('}');
    out.push('\n');
    out
}

pub fn write_record_map(record: &Record<CborValue>) -> String {
    let mut out = String::from("{");
    let mut first = true;
    for (k, v) in record.iter() {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&write_string(k));
        out.push(':');
        out.push_str(&write_value(v));
    }
    out.push('}');
    out
}

pub fn write_value(v: &CborValue) -> String {
    match v {
        CborValue::Integer(i) => write_integer(*i),
        CborValue::Float(f) => write_float(*f),
        CborValue::Text(s) => write_string(s),
        CborValue::Bytes(b) => write_bytes(b),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Null => "null".to_string(),
        CborValue::Array(items) => write_array(items),
        CborValue::Map(pairs) => write_map(pairs),
        CborValue::Tag(_, inner) => write_value(inner),
        other => write_string(&format!("{other:?}")),
    }
}

// Wider integers are quoted: JSON parsers typically round past f64's 53-bit mantissa.
fn write_integer(i: ciborium::value::Integer) -> String {
    let n = i128::from(i);
    if (i64::MIN as i128..=i64::MAX as i128).contains(&n) {
        n.to_string()
    } else {
        format!("\"{n}\"")
    }
}

fn write_float(f: f64) -> String {
    if f.is_nan() || f.is_infinite() {
        "null".to_string()
    } else {
        f.to_string()
    }
}

fn write_bytes(b: &[u8]) -> String {
    let mut hex = String::with_capacity(b.len() * 2 + 4);
    hex.push_str("\"0x");
    for byte in b {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex.push('"');
    hex
}

fn write_array(items: &[CborValue]) -> String {
    let mut out = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&write_value(item));
    }
    out.push(']');
    out
}

fn write_map(pairs: &[(CborValue, CborValue)]) -> String {
    let mut out = String::from("{");
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&write_map_key(k));
        out.push(':');
        out.push_str(&write_value(v));
    }
    out.push('}');
    out
}

/// Non-text keys are stringified through `write_value`.
fn write_map_key(k: &CborValue) -> String {
    match k {
        CborValue::Text(s) => write_string(s),
        other => write_string(&write_value(other)),
    }
}

fn write_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}
