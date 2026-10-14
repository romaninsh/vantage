//! Newline-delimited JSON — one record per line, easy to pipe to `jq -c`
//! or read line-by-line in tests. Same lossy rules as `json` (see that
//! module).

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_types::Record;

use super::json;

pub fn write_list(records: &IndexMap<String, Record<CborValue>>) -> String {
    let mut out = String::new();
    for (id, record) in records {
        out.push_str(&line(id, record));
    }
    out
}

pub fn write_record(id: &str, record: &Record<CborValue>) -> String {
    line(id, record)
}

pub fn write_scalar(label: &str, value: &CborValue) -> String {
    let mut out = String::from("{");
    out.push_str(&serde_json::to_string(label).unwrap_or_else(|_| "\"\"".to_string()));
    out.push(':');
    out.push_str(&json::write_value(value));
    out.push_str("}\n");
    out
}

fn line(id: &str, record: &Record<CborValue>) -> String {
    let mut out = String::from("{");
    // `_id`, not `id`: records usually carry their own typed `id` field.
    out.push_str(&serde_json::to_string("_id").unwrap_or_else(|_| "\"_id\"".to_string()));
    out.push(':');
    out.push_str(&serde_json::to_string(id).unwrap_or_else(|_| "\"\"".to_string()));
    for (k, v) in record.iter() {
        out.push(',');
        out.push_str(&serde_json::to_string(k).unwrap_or_else(|_| "\"\"".to_string()));
        out.push(':');
        out.push_str(&json::write_value(v));
    }
    out.push_str("}\n");
    out
}
