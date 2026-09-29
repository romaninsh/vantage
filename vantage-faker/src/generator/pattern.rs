//! `Pattern` expansion: `#` digit, `?` uppercase letter, `*` either, `\`
//! escapes the next character.

use fake::rand::RngExt as _;
use fake::rand::rngs::StdRng;

const DIGITS: &[u8] = b"0123456789";
const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

pub(crate) fn expand(rng: &mut StdRng, template: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars();
    while let Some(c) = chars.next() {
        let pool = match c {
            '#' => DIGITS,
            '?' => LETTERS,
            '*' => ALNUM,
            '\\' => {
                // A trailing backslash stays literal.
                out.push(chars.next().unwrap_or('\\'));
                continue;
            }
            other => {
                out.push(other);
                continue;
            }
        };
        out.push(char::from(pool[rng.random_range(0..pool.len())]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fake::rand::SeedableRng as _;

    #[test]
    fn pattern_shape_matches_template() {
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..200 {
            let s = expand(&mut rng, "BA-####");
            assert_eq!(s.len(), 7);
            assert!(s.starts_with("BA-"));
            assert!(s[3..].chars().all(|c| c.is_ascii_digit()), "{s}");

            let g = expand(&mut rng, "Gate ?##");
            let tail: Vec<char> = g["Gate ".len()..].chars().collect();
            assert!(tail[0].is_ascii_uppercase() && tail[1..].iter().all(char::is_ascii_digit));

            let a = expand(&mut rng, "G-****");
            assert!(
                a[2..]
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            );
        }
    }

    #[test]
    fn backslash_escapes_placeholders() {
        let mut rng = StdRng::seed_from_u64(1);
        let s = expand(&mut rng, r"\#\?\*\\#");
        assert_eq!(&s[..4], r"#?*\");
        assert!(s[4..].chars().all(|c| c.is_ascii_digit()));
        assert_eq!(expand(&mut rng, r"end\"), r"end\");
    }
}
