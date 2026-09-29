//! Realistic value generation for a faker column.
//!
//! A column's explicit [`ColumnGen`] wins; otherwise a two-tier strategy
//! matches the column *name* against common patterns first (an `email` column
//! gets a real email, `city` a city, …), then falls back to the declared
//! *type*. All realistic values come from the third-party `fake` crate — we
//! never hand-maintain name lists.
//!
//! The generator owns its rng. [`ValueGen::new`] seeds from the OS (the
//! classic behavior); [`ValueGen::seeded`] makes every draw a pure function
//! of the seed, so a scenario replays identically — the property the shaped
//! backends and their tests depend on.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use fake::Fake;
use fake::faker::address::en::{CityName, CountryName, StreetName};
use fake::faker::company::en::CompanyName;
use fake::faker::internet::en::{SafeEmail, Username};
use fake::faker::lorem::en::Word;
use fake::faker::name::en::{FirstName, LastName, Name};
use fake::faker::phone_number::en::PhoneNumber;
use fake::rand::rngs::StdRng;
use fake::rand::{RngExt as _, SeedableRng as _};
use vantage_types::Record;

use crate::FakerColumn;
use crate::generator::{self, Cell, ColumnGen, Memo, column_salt, now_unix};

/// Rows this fraction of `record_for` calls draw a value from the anomaly
/// pool instead of the realistic generator — see [`ValueGen::with_weirdness`].
const ANOMALIES: [&str; 4] = ["long", "blank", "duplicate", "unicode"];

/// A fresh rng seeded from the thread rng — the "no seed given" path.
pub(crate) fn entropy_rng() -> StdRng {
    StdRng::seed_from_u64(fake::rand::random())
}

/// Column-aware value generator with an owned, optionally-seeded rng.
#[derive(Clone)]
pub struct ValueGen {
    /// Shared across clones so a table's seed produces ONE deterministic
    /// stream regardless of how many handles draw from it.
    rng: Arc<Mutex<StdRng>>,
    /// Fraction of string cells drawn from the anomaly pool (0.0 = never).
    weirdness: f64,
    /// Seed of the positional generators (walk, tree, even dates): the seed
    /// itself, or entropy for an unseeded generator.
    salt: u64,
    /// Rows in the table being generated, if known — see [`Self::with_rows`].
    rows: Option<usize>,
    /// Unix seconds that relative dates resolve against.
    now: i64,
    /// Walk series and tree plans, shared across clones.
    memo: Arc<Mutex<Memo>>,
    /// Row counter for [`Self::record_for`], which is not told a `seq`.
    next_seq: Arc<AtomicUsize>,
}

impl Default for ValueGen {
    fn default() -> Self {
        Self::new()
    }
}

impl ValueGen {
    fn with_rng(rng: StdRng, salt: u64) -> Self {
        Self {
            rng: Arc::new(Mutex::new(rng)),
            weirdness: 0.0,
            salt,
            rows: None,
            now: now_unix(),
            memo: Arc::default(),
            next_seq: Arc::default(),
        }
    }

    /// Entropy-seeded: fresh values every run, like the thread-rng original.
    pub fn new() -> Self {
        Self::with_rng(entropy_rng(), fake::rand::random())
    }

    /// Deterministic: every draw is a pure function of `seed`. Relative dates
    /// still move with the wall clock unless pinned with [`Self::with_now`].
    pub fn seeded(seed: u64) -> Self {
        Self::with_rng(StdRng::seed_from_u64(seed), seed)
    }

    /// [`seeded`](Self::seeded) when `seed` is given, [`new`](Self::new)
    /// otherwise — the shape a config's optional `seed:` arrives in.
    pub fn from_seed(seed: Option<u64>) -> Self {
        seed.map_or_else(Self::new, Self::seeded)
    }

    /// Declare how many rows the table will have. Even-spread dates and trees
    /// scale to it; without it they assume 100 rows. The returned clone
    /// shares the rng stream and memo with `self`.
    pub fn with_rows(mut self, rows: usize) -> Self {
        self.rows = Some(rows);
        self
    }

    /// Seed for stateless per-column draws (see `generator::hash`).
    pub(crate) fn column_salt(&self, column: &str) -> u64 {
        column_salt(self.salt, column)
    }

    /// Pin the instant that `now` and relative offsets (`-90d`) resolve to,
    /// in unix seconds. Defaults to the moment the generator was created.
    pub fn with_now(mut self, unix_secs: i64) -> Self {
        self.now = unix_secs;
        self
    }

    /// Make this fraction of string cells anomalous: ~200-char labels, blank
    /// titles, a fixed duplicate name, unicode/emoji. Rendering oddities then
    /// appear *somewhere* in any big list without a dedicated fixture table.
    pub fn with_weirdness(mut self, weirdness: f64) -> Self {
        self.weirdness = weirdness.clamp(0.0, 1.0);
        self
    }

    /// Generate a single value appropriate for `col`: its generator if set,
    /// else name-pattern first. Positional generators read the next
    /// [`record_for`](Self::record_for) row number as their `seq`.
    pub fn value_for(&self, col: &FakerColumn) -> CborValue {
        let rng = &mut *self.rng.lock().unwrap();
        self.cell_value(rng, col, self.next_seq.load(Ordering::Relaxed))
    }

    fn cell_value(&self, rng: &mut StdRng, col: &FakerColumn, seq: usize) -> CborValue {
        match &col.generator {
            Some(generator) => self.generated(rng, generator, col, seq),
            None => Self::value_for_with(rng, col),
        }
    }

    fn generated(
        &self,
        rng: &mut StdRng,
        generator: &ColumnGen,
        col: &FakerColumn,
        seq: usize,
    ) -> CborValue {
        generator::generate(
            generator,
            Cell {
                rng,
                memo: &self.memo,
                salt: self.column_salt(&col.name),
                column: &col.name,
                ty: &col.ty,
                seq,
                rows: self.rows.unwrap_or(generator::DEFAULT_ROWS),
                now: self.now,
            },
        )
    }

    pub(crate) fn value_for_with(rng: &mut StdRng, col: &FakerColumn) -> CborValue {
        let name = col.name.to_lowercase();

        // --- name-aware ------------------------------------------------------
        if name.contains("email") {
            return CborValue::Text(SafeEmail().fake_with_rng(rng));
        }
        if name.contains("first") && name.contains("name") {
            return CborValue::Text(FirstName().fake_with_rng(rng));
        }
        if name.contains("last") && name.contains("name") || name.contains("surname") {
            return CborValue::Text(LastName().fake_with_rng(rng));
        }
        if name.contains("username") || name.contains("login") || name.contains("handle") {
            return CborValue::Text(Username().fake_with_rng(rng));
        }
        if name.contains("name") {
            return CborValue::Text(Name().fake_with_rng(rng));
        }
        if name.contains("phone") || name.contains("mobile") || name.contains("tel") {
            return CborValue::Text(PhoneNumber().fake_with_rng(rng));
        }
        if name.contains("city") {
            return CborValue::Text(CityName().fake_with_rng(rng));
        }
        if name.contains("country") {
            return CborValue::Text(CountryName().fake_with_rng(rng));
        }
        if name.contains("street") || name.contains("address") {
            return CborValue::Text(StreetName().fake_with_rng(rng));
        }
        if name.contains("company") || name.contains("employer") || name.contains("organization") {
            return CborValue::Text(CompanyName().fake_with_rng(rng));
        }

        // --- type fallback ---------------------------------------------------
        Self::value_by_type(rng, &col.ty)
    }

    fn value_by_type(rng: &mut StdRng, ty: &str) -> CborValue {
        match ty.trim().to_lowercase().as_str() {
            "int" | "integer" | "number" | "i64" | "bigint" => {
                let n: i64 = (0..10_000).fake_with_rng(rng);
                CborValue::Integer(n.into())
            }
            "decimal" | "float" | "double" | "money" | "amount" | "f64" => {
                // two-decimal money-like value without a float-formatting dep
                let cents: i64 = (0..1_000_000).fake_with_rng(rng);
                CborValue::Float(cents as f64 / 100.0)
            }
            "bool" | "boolean" => CborValue::Bool(fake::Faker.fake_with_rng(rng)),
            "datetime" | "date" | "timestamp" => {
                // avoid a wall-clock / chrono dependency — vary a plausible ISO string
                let day: u8 = (1..=28).fake_with_rng(rng);
                let hour: u8 = (0..24).fake_with_rng(rng);
                CborValue::Text(format!("2026-01-{day:02}T{hour:02}:00:00Z"))
            }
            // string and anything unknown
            _ => CborValue::Text(Word().fake_with_rng(rng)),
        }
    }

    /// One draw from the anomaly pool — always a string, because the pool
    /// exists to stress *label rendering*.
    fn anomaly(rng: &mut StdRng) -> CborValue {
        let kind = ANOMALIES[rng.random_range(0..ANOMALIES.len())];
        CborValue::Text(match kind {
            "long" => {
                let mut s = String::with_capacity(210);
                while s.len() < 200 {
                    let w: String = Word().fake_with_rng(rng);
                    s.push_str(&w);
                    s.push(' ');
                }
                s
            }
            "blank" => String::new(),
            "duplicate" => "John Smith".to_string(),
            _ => "Žofia 🌸 Ōkami".to_string(),
        })
    }

    /// Build a full record for `id`, filling every column. The id column is set
    /// to `id` verbatim; all others are generated — each string cell of a
    /// column without a generator standing a `weirdness` chance of drawing
    /// from the anomaly pool instead.
    ///
    /// Positional generators see the number of `record_for` calls made so far
    /// on this generator (and its clones) as the row's `seq`; use
    /// [`record_at`](Self::record_at) to say it explicitly.
    pub fn record_for(
        &self,
        columns: &[FakerColumn],
        id_column: &str,
        id: &str,
    ) -> Record<CborValue> {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        self.record_at(columns, id_column, id, seq)
    }

    /// [`record_for`](Self::record_for) for row `seq` of the table: walks,
    /// trees and even-spread dates take their position from `seq`. Columns
    /// without a generator draw exactly as `record_for` does, so the rng
    /// stream — and a seeded table's output — is unchanged by the choice.
    pub fn record_at(
        &self,
        columns: &[FakerColumn],
        id_column: &str,
        id: &str,
        seq: usize,
    ) -> Record<CborValue> {
        let rng = &mut *self.rng.lock().unwrap();
        let mut rec = Record::new();
        let mut wrote_id = false;
        for col in columns {
            if col.name == id_column {
                rec.insert(col.name.clone(), CborValue::Text(id.to_string()));
                wrote_id = true;
            } else if let Some(generator) = &col.generator {
                let value = self.generated(rng, generator, col, seq);
                rec.insert(col.name.clone(), value);
            } else {
                let mut value = Self::value_for_with(rng, col);
                if self.weirdness > 0.0
                    && matches!(value, CborValue::Text(_))
                    && rng.random_range(0.0..1.0) < self.weirdness
                {
                    value = Self::anomaly(rng);
                }
                rec.insert(col.name.clone(), value);
            }
        }
        if !wrote_id {
            rec.insert(id_column.to_string(), CborValue::Text(id.to_string()));
        }
        rec
    }
}

#[cfg(test)]
mod tests;
