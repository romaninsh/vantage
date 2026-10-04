//! Column names as words, for the name-aware guess in [`ValueGen`](super::ValueGen).
//!
//! Matching whole words rather than substrings keeps `hostname` and
//! `filename` from being person names and `hotel` from being a phone number.

/// The lowercase words a column name is made of: split on `_`, `-`, `.` (and
/// any other non-alphanumeric) and at camelCase boundaries, so `lastName`,
/// `last_name` and `last-name` all give `last`, `name`. Every pair of adjacent
/// words is appended joined together (`lastname`), which is how a lowercase
/// `lastname` or `username` column matches the same rule as `last_name`.
pub(super) fn name_words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            flush(&mut tokens, &mut current);
            continue;
        }
        if c.is_uppercase() && !current.is_empty() {
            let prev = chars[i - 1];
            let next_is_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            // `userName` splits before `N`; `HTTPServer` splits before `S`
            // (the last capital of a run that starts a new lowercase word).
            if prev.is_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_uppercase() && next_is_lower)
            {
                flush(&mut tokens, &mut current);
            }
        }
        current.extend(c.to_lowercase());
    }
    flush(&mut tokens, &mut current);

    let pairs: Vec<String> = tokens
        .windows(2)
        .map(|w| format!("{}{}", w[0], w[1]))
        .collect();
    tokens.extend(pairs);
    tokens
}

fn flush(tokens: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(std::mem::take(current));
    }
}

/// Whether any of `candidates` is one of `words`.
pub(super) fn has_any(words: &[String], candidates: &[&str]) -> bool {
    candidates.iter().any(|c| words.iter().any(|w| w == c))
}
