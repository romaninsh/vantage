//! The one `${ … }` scanner. Brace-nesting aware and string-literal aware.

use crate::error::{Located, Result, RhaiError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Lit(String),
    /// The text between `${` and its matching `}`, untrimmed.
    Hole(String),
}

/// Split a template into literal text and holes.
pub fn split(src: &str) -> Result<Vec<Part>> {
    let mut parts = Vec::new();
    let mut rest = src;
    let mut offset = 0usize;
    while let Some(start) = rest.find("${") {
        if start > 0 {
            parts.push(Part::Lit(rest[..start].to_string()));
        }
        let after = &rest[start + 2..];
        let Some(end) = close_brace(after) else {
            return Err(unterminated(src, offset + start));
        };
        parts.push(Part::Hole(after[..end].to_string()));
        offset += start + 2 + end + 1;
        rest = &after[end + 1..];
    }
    if !rest.is_empty() {
        parts.push(Part::Lit(rest.to_string()));
    }
    Ok(parts)
}

/// `${ expr }` around the whole string → `expr`. Two holes (`${a}${b}`) are
/// not a wrapper and come back unchanged so the caller's compile reports the
/// syntax error.
pub fn strip_single_wrapper(src: &str) -> &str {
    let trimmed = src.trim();
    match trimmed.strip_prefix("${").and_then(|r| r.strip_suffix('}')) {
        Some(inner) if close_brace(&trimmed[2..]) == Some(trimmed.len() - 3) => inner,
        _ => trimmed,
    }
}

/// Byte index of the `}` that closes a hole whose `${` has already been
/// consumed. Counts nested braces, and skips anything rhai would not read as
/// code: `"…"` and `'…'` (with backslash escapes), backtick literal strings,
/// and `//` / `/* */` comments. A brace inside any of those is text, not
/// structure.
fn close_brace(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            // Escaped strings: `"…"` and the char literal `'…'`.
            q @ (b'"' | b'\'') => {
                i = skip_escaped(b, i, q)?;
                continue;
            }
            // Rhai's literal string. No backslash escapes inside, so the next
            // backtick always closes it; `${…}` interpolation is skipped whole.
            b'`' => {
                i = memchr(b, i + 1, b'`')? + 1;
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                // Line comment: to the newline. Running off the end means the
                // hole was never closed.
                i = memchr(b, i + 2, b'\n')? + 1;
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut j = i + 2;
                loop {
                    j = memchr(b, j, b'*')?;
                    if b.get(j + 1) == Some(&b'/') {
                        i = j + 2;
                        break;
                    }
                    j += 1;
                }
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Index just past the closing `quote`, honouring backslash escapes.
fn skip_escaped(b: &[u8], open: usize, quote: u8) -> Option<usize> {
    let mut i = open + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            c if c == quote => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

fn memchr(b: &[u8], from: usize, needle: u8) -> Option<usize> {
    (from..b.len()).find(|&i| b[i] == needle)
}

fn unterminated(src: &str, at: usize) -> RhaiError {
    let line = src[..at].matches('\n').count() + 1;
    // Char column, not byte: `Located::Display` counts spaces, and rhai's own
    // positions are char-based, so the two sources must agree.
    let line_start = src[..at].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let column = src[line_start..at].chars().count() + 1;
    RhaiError::Syntax(Located {
        src: src.to_string(),
        line,
        column,
        message: format!("unterminated `${{` at byte {at}"),
    })
}
