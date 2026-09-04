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

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(s: &str) -> Part {
        Part::Lit(s.to_string())
    }
    fn hole(s: &str) -> Part {
        Part::Hole(s.to_string())
    }

    #[test]
    fn no_holes_is_one_literal() {
        assert_eq!(split("Hello").unwrap(), vec![lit("Hello")]);
        assert_eq!(split("").unwrap(), vec![]);
    }

    #[test]
    fn mixed_text_and_holes() {
        assert_eq!(
            split("Order ${args.id} in ${cur}").unwrap(),
            vec![lit("Order "), hole("args.id"), lit(" in "), hole("cur")]
        );
    }

    #[test]
    fn nested_braces_survive() {
        assert_eq!(
            split(r#"${ if x { "a" } else { "b" } }!"#).unwrap(),
            vec![hole(r#" if x { "a" } else { "b" } "#), lit("!")]
        );
    }

    #[test]
    fn braces_inside_string_literals_are_ignored() {
        assert_eq!(
            split(r#"${ "}" + '{' + "\"}" }"#).unwrap(),
            vec![hole(r#" "}" + '{' + "\"}" "#)]
        );
    }

    #[test]
    fn backtick_literal_strings_are_skipped() {
        // Rhai's third string form. A brace inside one is text, and the engine
        // accepts these — the scanner must not split on them.
        assert_eq!(split("${ `{` }").unwrap(), vec![hole(" `{` ")]);
        assert_eq!(split("${ `}` }").unwrap(), vec![hole(" `}` ")]);
        assert_eq!(
            split(r#"${ "a" + `b}c` }"#).unwrap(),
            vec![hole(r#" "a" + `b}c` "#)]
        );
        // Interpolation inside a backtick string carries its own braces.
        assert_eq!(split("${ `x${y}z` }").unwrap(), vec![hole(" `x${y}z` ")]);
    }

    #[test]
    fn comments_inside_a_hole_are_skipped() {
        assert_eq!(
            split("${ 1 /* } */ + 2 }").unwrap(),
            vec![hole(" 1 /* } */ + 2 ")]
        );
        assert_eq!(
            split("${ 1 + // }\n 2 }").unwrap(),
            vec![hole(" 1 + // }\n 2 ")]
        );
    }

    #[test]
    fn char_literal_braces_are_skipped() {
        assert_eq!(split("${ '}' }").unwrap(), vec![hole(" '}' ")]);
        assert_eq!(
            split(r"${ '\'' + '{' }").unwrap(),
            vec![hole(r" '\'' + '{' ")]
        );
    }

    #[test]
    fn unterminated_hole_column_counts_chars_not_bytes() {
        // "café " is 5 chars but 6 bytes; the caret must land on the `$`.
        let err = split("café ${x").unwrap_err();
        let RhaiError::Syntax(l) = err else { panic!() };
        assert_eq!((l.line, l.column), (1, 6));
    }

    #[test]
    fn unterminated_hole_reports_position() {
        let err = split("a\nbb ${x").unwrap_err();
        let RhaiError::Syntax(l) = err else { panic!() };
        assert_eq!((l.line, l.column), (2, 4));
        assert!(l.message.contains("unterminated"));
    }

    #[test]
    fn single_wrapper_is_stripped() {
        assert_eq!(strip_single_wrapper("${ a + b }"), " a + b ");
        assert_eq!(strip_single_wrapper("  a + b  "), "a + b");
        assert_eq!(strip_single_wrapper("${a}${b}"), "${a}${b}");
        assert_eq!(
            strip_single_wrapper("${ if x { 1 } else { 2 } }"),
            " if x { 1 } else { 2 } "
        );
    }
}
