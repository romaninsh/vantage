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
    // The IndexMap key is reported under `_id` (sentinel-prefixed) to
    // sidestep clashes with the record's own `id` field. Records
    // typically carry their own `id` which is the authoritative typed
    // value; `_id` is only a stable framing handle.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_per_record() {
        let mut records: IndexMap<String, Record<CborValue>> = IndexMap::new();
        let mut r1 = Record::new();
        r1.insert("x".to_string(), CborValue::Integer(1.into()));
        records.insert("a".to_string(), r1);
        let mut r2 = Record::new();
        r2.insert("x".to_string(), CborValue::Integer(2.into()));
        records.insert("b".to_string(), r2);
        let s = write_list(&records);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "{\"_id\":\"a\",\"x\":1}");
        assert_eq!(lines[1], "{\"_id\":\"b\",\"x\":2}");
    }

    #[test]
    fn scalar_one_line() {
        let s = write_scalar("count", &CborValue::Integer(5.into()));
        assert_eq!(s, "{\"count\":5}\n");
    }
}
