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
/// consumed. Counts nested braces; ignores braces inside `"…"` / `'…'`
/// literals, honouring backslash escapes.
fn close_brace(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 1;
                } else if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'{' => depth += 1,
                b'}' => {
                    if depth == 0 {
                        return Some(i);
                    }
                    depth -= 1;
                }
                _ => {}
            },
        }
        i += 1;
    }
    None
}

fn unterminated(src: &str, at: usize) -> RhaiError {
    let line = src[..at].matches('\n').count() + 1;
    let column = at - src[..at].rfind('\n').map(|p| p + 1).unwrap_or(0) + 1;
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
