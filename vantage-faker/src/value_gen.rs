//! Realistic value generation for a faker column.
//!
//! Two-tier strategy: match the column *name* against common patterns first
//! (an `email` column gets a real email, `city` a city, …), then fall back to
//! the declared *type*. All values come from the third-party `fake` crate — we
//! never hand-maintain name lists.
//!
//! The generator owns its rng. [`ValueGen::new`] seeds from the OS (the
//! classic behavior); [`ValueGen::seeded`] makes every draw a pure function
//! of the seed, so a scenario replays identically — the property the shaped
//! backends and their tests depend on.

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
}

impl Default for ValueGen {
    fn default() -> Self {
        Self::new()
    }
}

impl ValueGen {
    /// Entropy-seeded: fresh values every run, like the thread-rng original.
    pub fn new() -> Self {
        Self {
            rng: Arc::new(Mutex::new(entropy_rng())),
            weirdness: 0.0,
        }
    }

    /// Deterministic: every draw is a pure function of `seed`.
    pub fn seeded(seed: u64) -> Self {
        Self {
            rng: Arc::new(Mutex::new(StdRng::seed_from_u64(seed))),
            weirdness: 0.0,
        }
    }

    /// Make this fraction of string cells anomalous: ~200-char labels, blank
    /// titles, a fixed duplicate name, unicode/emoji. Rendering oddities then
    /// appear *somewhere* in any big list without a dedicated fixture table.
    pub fn with_weirdness(mut self, weirdness: f64) -> Self {
        self.weirdness = weirdness.clamp(0.0, 1.0);
        self
    }

    /// Generate a single value appropriate for `col`, name-pattern first.
    pub fn value_for(&self, col: &FakerColumn) -> CborValue {
        let rng = &mut *self.rng.lock().unwrap();
        Self::value_for_with(rng, col)
    }

    fn value_for_with(rng: &mut StdRng, col: &FakerColumn) -> CborValue {
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
    /// to `id` verbatim; all others are generated — each string cell standing a
    /// `weirdness` chance of drawing from the anomaly pool instead.
    pub fn record_for(
        &self,
        columns: &[FakerColumn],
        id_column: &str,
        id: &str,
    ) -> Record<CborValue> {
        let rng = &mut *self.rng.lock().unwrap();
        let mut rec = Record::new();
        let mut wrote_id = false;
        for col in columns {
            if col.name == id_column {
                rec.insert(col.name.clone(), CborValue::Text(id.to_string()));
                wrote_id = true;
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
mod tests {
    use super::*;

    fn col(name: &str, ty: &str) -> FakerColumn {
        FakerColumn {
            name: name.to_string(),
            ty: ty.to_string(),
            flags: vec![],
        }
    }

    fn text(v: &CborValue) -> &str {
        match v {
            CborValue::Text(s) => s,
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn email_column_is_email_shaped() {
        let g = ValueGen::new();
        let v = g.value_for(&col("email", "string"));
        assert!(text(&v).contains('@'), "expected an email, got {v:?}");
    }

    #[test]
    fn name_column_is_nonempty_text() {
        let g = ValueGen::new();
        let v = g.value_for(&col("full_name", "string"));
        assert!(!text(&v).is_empty());
    }

    #[test]
    fn type_fallback_maps_scalars() {
        let g = ValueGen::new();
        assert!(matches!(
            g.value_for(&col("qty", "int")),
            CborValue::Integer(_)
        ));
        assert!(matches!(
            g.value_for(&col("balance", "decimal")),
            CborValue::Float(_)
        ));
        assert!(matches!(
            g.value_for(&col("active", "bool")),
            CborValue::Bool(_)
        ));
    }

    #[test]
    fn record_sets_id_column_and_fills_rest() {
        let g = ValueGen::new();
        let cols = [
            col("id", "string"),
            col("email", "string"),
            col("age", "int"),
        ];
        let rec = g.record_for(&cols, "id", "abc");
        assert_eq!(rec.get("id"), Some(&CborValue::Text("abc".into())));
        assert!(text(rec.get("email").unwrap()).contains('@'));
        assert!(matches!(rec.get("age"), Some(CborValue::Integer(_))));
    }

    #[test]
    fn same_seed_replays_the_same_records() {
        let cols = [col("id", "string"), col("name", "string"), col("age", "int")];
        let a: Vec<_> = {
            let g = ValueGen::seeded(42);
            (0..5).map(|i| g.record_for(&cols, "id", &i.to_string())).collect()
        };
        let b: Vec<_> = {
            let g = ValueGen::seeded(42);
            (0..5).map(|i| g.record_for(&cols, "id", &i.to_string())).collect()
        };
        assert_eq!(a, b, "a seed is a promise");
    }

    #[test]
    fn weirdness_one_makes_every_string_cell_anomalous() {
        let g = ValueGen::seeded(7).with_weirdness(1.0);
        let cols = [col("id", "string"), col("name", "string")];
        let anomalies: Vec<String> = (0..40)
            .map(|i| text(g.record_for(&cols, "id", &i.to_string()).get("name").unwrap()).to_string())
            .collect();
        // Every value came from the pool: blank, John Smith, unicode, or long.
        for v in &anomalies {
            let from_pool = v.is_empty()
                || v == "John Smith"
                || v.contains('Ž')
                || v.len() >= 200;
            assert!(from_pool, "unexpected non-anomalous value {v:?}");
        }
        // And the pool cycles — more than one kind appears over 40 draws.
        assert!(anomalies.iter().any(|v| v.is_empty()));
        assert!(anomalies.iter().any(|v| v.len() >= 200));
    }

    #[test]
    fn weirdness_zero_never_draws_anomalies() {
        let g = ValueGen::seeded(7);
        let cols = [col("id", "string"), col("name", "string")];
        for i in 0..40 {
            let rec = g.record_for(&cols, "id", &i.to_string());
            let v = text(rec.get("name").unwrap()).to_string();
            assert!(!v.is_empty() && v != "John Smith" && v.len() < 200, "anomaly leaked: {v:?}");
        }
    }
}
