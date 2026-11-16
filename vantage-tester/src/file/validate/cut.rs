use std::iter::Peekable;
use std::str::Chars;

use vantage_core::{Result, error};

/// YAML reads ` #` as a comment, so an unquoted `set(acme, #{ … })` arrives as `set(acme,`.
/// Refuse Rhai whose brackets don't close or that ends in a comma, before the host sees it.
/// Brackets, quotes and commas inside `//` and `/* */` comments don't count.
pub(super) fn check_not_cut(scenario: &str, step: &str, rhai: &str) -> Result<()> {
    let mut depth = 0i32;
    let mut quote = None;
    let mut open_comment = false;
    let mut code = String::new();
    let mut chars = rhai.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(_), '\\') => {
                chars.next();
            }
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '/') if chars.next_if_eq(&'/').is_some() => {
                while chars.next_if(|c| *c != '\n').is_some() {}
                continue;
            }
            (None, '/') if chars.next_if_eq(&'*').is_some() => {
                open_comment = !skip_block_comment(&mut chars);
                continue;
            }
            (None, '"' | '\'' | '`') => quote = Some(c),
            (None, '(' | '[' | '{') => depth += 1,
            (None, ')' | ']' | '}') => depth -= 1,
            _ => {}
        }
        code.push(c);
    }
    if depth == 0 && quote.is_none() && !open_comment && !code.trim_end().ends_with(',') {
        return Ok(());
    }
    Err(error!(
        format!(
            "Rhai `{rhai}` looks cut short: in YAML ` #` starts a comment, so quote a value containing `#{{`"
        ),
        scenario = scenario,
        step = step
    ))
}

/// Skips past the end of a block comment whose `/*` was just read; Rhai nests them. `false`
/// when the text ends first.
fn skip_block_comment(chars: &mut Peekable<Chars>) -> bool {
    let mut depth = 1;
    while depth > 0 {
        match chars.next() {
            Some('*') if chars.next_if_eq(&'/').is_some() => depth -= 1,
            Some('/') if chars.next_if_eq(&'*').is_some() => depth += 1,
            Some(_) => {}
            None => return false,
        }
    }
    true
}
