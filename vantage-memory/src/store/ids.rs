//! Per-table id generation: a counter, optionally prefixed, that skips ids
//! already taken by caller-supplied rows.

use std::sync::atomic::{AtomicU64, Ordering};

use ciborium::Value as CborValue;

pub(crate) struct IdGen {
    next: AtomicU64,
    prefix: Option<String>,
}

impl IdGen {
    pub fn new(prefix: Option<String>) -> Self {
        Self {
            next: AtomicU64::new(1),
            prefix,
        }
    }

    /// The next id for which `taken` is false.
    pub fn next(&self, taken: impl Fn(&str) -> bool) -> String {
        loop {
            let n = self.next.fetch_add(1, Ordering::Relaxed);
            let id = match &self.prefix {
                Some(p) => format!("{p}{n}"),
                None => n.to_string(),
            };
            if !taken(&id) {
                return id;
            }
        }
    }
}

/// The cell values that can refer to row `id`: its text form and, when it
/// parses as an integer, its integer form. Seed files often hold foreign
/// keys as integers while stored ids are always text.
pub(crate) fn id_forms(id: &str) -> Vec<CborValue> {
    let mut forms = vec![CborValue::Text(id.to_string())];
    if let Ok(n) = id.parse::<i64>() {
        forms.push(CborValue::Integer(n.into()));
    }
    forms
}

/// The id a row supplies in its id column: non-empty text or an integer.
pub(crate) fn supplied_id(value: Option<&CborValue>) -> Option<String> {
    match value {
        Some(CborValue::Text(s)) if !s.is_empty() => Some(s.clone()),
        Some(CborValue::Integer(i)) => Some(i128::from(*i).to_string()),
        _ => None,
    }
}
