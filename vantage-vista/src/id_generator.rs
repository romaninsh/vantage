//! How a writer mints the id of a new record: [`IdGenerator`].
//!
//! The writer asks once, before the first attempt, and sends the same id on
//! every retry, so a retried insert lands on "already in the set" and returns
//! the stored row. A generator doesn't detect clashes: pick one strong enough
//! for the table's lifetime size.

mod shell;

use std::sync::Arc;

use vantage_core::{Result, error};

pub(crate) use shell::IdGeneratorShell;

/// How a missing id is minted when a new record is inserted.
///
/// `UuidV7` is the default: it is time-ordered (RFC 9562), so ids sort by
/// creation time and cluster together in a primary-key index instead of
/// scattering it the way the purely-random `UuidV4` does.
#[derive(Clone, Default)]
#[non_exhaustive]
pub enum IdGenerator {
    /// Time-ordered UUID (v7).
    #[default]
    UuidV7,
    /// Random UUID (v4).
    UuidV4,
    /// A short random id: `length` characters drawn uniformly from an
    /// alphabet. See [`ShortId`].
    Short(ShortId),
    /// Any caller-supplied scheme (ULID, nanoid, a counter, …).
    Custom(Arc<dyn Fn() -> String + Send + Sync>),
    /// A caller-supplied scheme that can fail (a compiled script). The
    /// write that asked for the id fails with its error.
    Fallible(Arc<dyn Fn() -> Result<String> + Send + Sync>),
}

impl IdGenerator {
    /// A custom generator from a closure.
    pub fn custom(f: impl Fn() -> String + Send + Sync + 'static) -> Self {
        IdGenerator::Custom(Arc::new(f))
    }

    /// A generator from a closure that can fail; see
    /// [`try_generate`](Self::try_generate).
    pub fn fallible(f: impl Fn() -> Result<String> + Send + Sync + 'static) -> Self {
        IdGenerator::Fallible(Arc::new(f))
    }

    /// A short id of `length` characters from `alphabet`; see [`ShortId::new`].
    pub fn short(length: usize, alphabet: &str) -> Result<Self> {
        ShortId::new(length, alphabet).map(IdGenerator::Short)
    }

    /// One fresh id, or the error of a [`Fallible`](Self::Fallible)
    /// generator. Writers mint through this.
    pub fn try_generate(&self) -> Result<String> {
        match self {
            IdGenerator::Fallible(f) => f(),
            other => Ok(other.generate()),
        }
    }

    /// One fresh id.
    ///
    /// # Panics
    ///
    /// When a [`Fallible`](Self::Fallible) generator fails; use
    /// [`try_generate`](Self::try_generate) for those.
    pub fn generate(&self) -> String {
        match self {
            IdGenerator::UuidV7 => uuid::Uuid::now_v7().to_string(),
            IdGenerator::UuidV4 => uuid::Uuid::new_v4().to_string(),
            IdGenerator::Short(short) => short.generate(),
            IdGenerator::Custom(f) => f(),
            IdGenerator::Fallible(f) => f().unwrap_or_else(|e| panic!("id generator failed: {e}")),
        }
    }
}

impl std::fmt::Debug for IdGenerator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdGenerator::UuidV7 => f.write_str("IdGenerator::UuidV7"),
            IdGenerator::UuidV4 => f.write_str("IdGenerator::UuidV4"),
            IdGenerator::Short(short) => f.debug_tuple("IdGenerator::Short").field(short).finish(),
            IdGenerator::Custom(_) => f.write_str("IdGenerator::Custom"),
            IdGenerator::Fallible(_) => f.write_str("IdGenerator::Fallible"),
        }
    }
}

/// A short random id: a fixed length over a fixed alphabet.
///
/// Every character is drawn uniformly from the OS random source. A clash
/// isn't detected (the second writer gets the first record back), so size it
/// for the table: at base58, 10 characters keep a million rows clash-free
/// with odds of about one in a million.
#[derive(Clone, PartialEq, Eq)]
pub struct ShortId {
    length: usize,
    alphabet: Arc<[char]>,
}

impl ShortId {
    /// Bitcoin's base58: letters and digits without `0`, `O`, `I` and `l`.
    pub const BASE58: &'static str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    /// `A`–`Z`, `a`–`z` and `0`–`9`.
    pub const ALNUM: &'static str =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    /// The longest id a `ShortId` makes.
    pub const MAX_LENGTH: usize = 256;

    /// `length` characters (1..=[`MAX_LENGTH`](Self::MAX_LENGTH)) from
    /// `alphabet`, which needs at least two characters, none repeated and
    /// none whitespace or control.
    pub fn new(length: usize, alphabet: &str) -> Result<Self> {
        if length == 0 || length > Self::MAX_LENGTH {
            return Err(error!(
                format!("short id length must be 1..={}", Self::MAX_LENGTH),
                length = length
            ));
        }
        let chars: Vec<char> = alphabet.chars().collect();
        if chars.len() < 2 {
            return Err(error!(
                "short id alphabet needs at least two characters",
                alphabet = alphabet
            ));
        }
        if let Some(c) = chars.iter().find(|c| c.is_whitespace() || c.is_control()) {
            return Err(error!(
                "short id alphabet holds whitespace or a control character",
                character = c
            ));
        }
        for (i, c) in chars.iter().enumerate() {
            if chars[..i].contains(c) {
                return Err(error!(
                    "short id alphabet repeats a character",
                    character = c
                ));
            }
        }
        Ok(Self {
            length,
            alphabet: chars.into(),
        })
    }

    pub fn length(&self) -> usize {
        self.length
    }

    pub fn alphabet(&self) -> String {
        self.alphabet.iter().collect()
    }

    /// One fresh id. Rejection sampling keeps every character uniform: a
    /// draw past the last whole multiple of the alphabet size is redrawn.
    pub fn generate(&self) -> String {
        let n = self.alphabet.len() as u64;
        let limit = u64::MAX - u64::MAX % n;
        let mut out = String::with_capacity(self.length);
        let mut made = 0;
        while made < self.length {
            let draw = getrandom::u64().expect("the OS random source is unavailable");
            if draw < limit {
                out.push(self.alphabet[(draw % n) as usize]);
                made += 1;
            }
        }
        out
    }
}

impl std::fmt::Debug for ShortId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShortId")
            .field("length", &self.length)
            .field("alphabet", &self.alphabet())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_ids_have_the_length_and_alphabet_asked_for() {
        let short = ShortId::new(10, ShortId::BASE58).unwrap();
        for _ in 0..200 {
            let id = short.generate();
            assert_eq!(id.chars().count(), 10, "{id}");
            assert!(id.chars().all(|c| ShortId::BASE58.contains(c)), "{id}");
        }
    }

    #[test]
    fn short_ids_reach_every_character_of_a_small_alphabet() {
        let short = ShortId::new(64, "ab").unwrap();
        let id = short.generate();
        assert!(id.contains('a') && id.contains('b'), "{id}");
    }

    #[test]
    fn base58_has_58_characters_without_lookalikes() {
        assert_eq!(ShortId::BASE58.chars().count(), 58);
        assert!(!ShortId::BASE58.contains(&['0', 'O', 'I', 'l'][..]));
        assert_eq!(ShortId::ALNUM.chars().count(), 62);
    }

    #[test]
    fn short_id_refuses_bad_lengths_and_alphabets() {
        for (length, alphabet) in [
            (0, ShortId::BASE58),
            (257, ShortId::BASE58),
            (8, "a"),
            (8, "abca"),
            (8, "ab c"),
        ] {
            assert!(
                ShortId::new(length, alphabet).is_err(),
                "{length} over {alphabet:?} accepted"
            );
        }
    }

    #[test]
    fn a_fallible_generator_reports_its_error_through_try_generate() {
        let failing = IdGenerator::fallible(|| Err(error!("no ids today")));
        let err = failing.try_generate().unwrap_err();
        assert!(err.to_string().contains("no ids today"), "{err}");
        let ok = IdGenerator::fallible(|| Ok("made".into()));
        assert_eq!(ok.try_generate().unwrap(), "made");
        assert_eq!(ok.generate(), "made");
        assert_eq!(
            IdGenerator::custom(|| "c".into()).try_generate().unwrap(),
            "c"
        );
    }

    #[test]
    fn the_default_generator_is_uuid_v7() {
        let id = IdGenerator::default().generate();
        assert_eq!(uuid::Uuid::parse_str(&id).unwrap().get_version_num(), 7);
    }
}
