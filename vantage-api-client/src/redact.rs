//! What a credential looks like once a URL, a header or an error text leaves
//! the process: always `***`. Every log line and error message built from
//! request data goes through here, so a new one has one place to get wrong.

use std::sync::Mutex;

/// The stand-in for a secret value.
pub(crate) const MASK: &str = "***";

/// The credentials of one request: query parameters whose values are masked
/// wherever a URL is shown, and literal values masked wherever free text is.
/// Built on demand and never stored, so it cannot leak through `Debug`.
///
/// The bearer token a request carries is only known once the request is
/// built (a source is asked per attempt), so [`Redaction::sent`] records it
/// as it goes out instead of asking the source a second time.
#[derive(Default)]
pub(crate) struct Redaction {
    params: Vec<String>,
    values: Vec<String>,
    sent: Mutex<Vec<String>>,
}

impl Redaction {
    /// Mask the value of query parameter `name` in every URL shown.
    pub(crate) fn param(mut self, name: impl Into<String>) -> Self {
        self.params.push(name.into());
        self
    }

    /// Mask `value` wherever it appears in free text.
    pub(crate) fn value(mut self, value: impl Into<String>) -> Self {
        let value = value.into();
        if !value.is_empty() {
            self.values.push(value);
        }
        self
    }

    /// Mask the `Authorization` header value that went out on an attempt,
    /// and the bare token inside a `Bearer` one.
    pub(crate) fn sent(&self, header: &str) {
        let token = header.strip_prefix("Bearer ").unwrap_or(header);
        let mut sent = self.sent.lock().unwrap_or_else(|e| e.into_inner());
        for value in [header, token] {
            if !value.is_empty() && !sent.iter().any(|v| v == value) {
                sent.push(value.to_string());
            }
        }
    }

    /// `url` with the value of each masked query parameter replaced.
    pub(crate) fn url(&self, url: &str) -> String {
        let Some((head, query)) = url.split_once('?') else {
            return url.to_string();
        };
        let query: Vec<String> = query
            .split('&')
            .map(|pair| match pair.split_once('=') {
                Some((name, _)) if self.masks_param(name) => format!("{name}={MASK}"),
                _ => pair.to_string(),
            })
            .collect();
        format!("{head}?{}", query.join("&"))
    }

    /// `text` with every masked value replaced, as written and as it would
    /// appear percent-encoded in a URL.
    pub(crate) fn text(&self, text: &str) -> String {
        let sent = self.sent.lock().unwrap_or_else(|e| e.into_inner());
        let mut needles: Vec<String> = self
            .values
            .iter()
            .chain(sent.iter())
            .flat_map(|v| [v.clone(), urlencoding::encode(v).into_owned()])
            .collect();
        needles.sort_by_key(|n| std::cmp::Reverse(n.len()));
        needles
            .iter()
            .fold(text.to_string(), |text, needle| text.replace(needle, MASK))
    }

    fn masks_param(&self, name: &str) -> bool {
        let name = urlencoding::decode(name).map_or_else(|_| name.into(), |n| n.into_owned());
        self.params.contains(&name)
    }
}

/// Whether a header's name says its value is a credential.
pub(crate) fn is_secret_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["auth", "key", "token", "secret", "cookie"]
        .iter()
        .any(|word| lower.contains(word))
}

/// One header as a log line shows it: credentials are `***`.
pub(crate) fn header_line(name: &str, value: &str) -> String {
    format!(
        "{name}: {}",
        if is_secret_header(name) { MASK } else { value }
    )
}
