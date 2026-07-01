//! Realistic value generation for a faker column.
//!
//! Two-tier strategy: match the column *name* against common patterns first
//! (an `email` column gets a real email, `city` a city, …), then fall back to
//! the declared *type*. All values come from the third-party `fake` crate — we
//! never hand-maintain name lists.

use ciborium::Value as CborValue;
use fake::Fake;
use fake::faker::address::en::{CityName, CountryName, StreetName};
use fake::faker::company::en::CompanyName;
use fake::faker::internet::en::{SafeEmail, Username};
use fake::faker::lorem::en::Word;
use fake::faker::name::en::{FirstName, LastName, Name};
use fake::faker::phone_number::en::PhoneNumber;
use vantage_types::Record;

use crate::FakerColumn;

/// Stateless generator — every call draws fresh values from `fake`'s thread rng.
#[derive(Clone, Default)]
pub struct ValueGen;

impl ValueGen {
    pub fn new() -> Self {
        Self
    }

    /// Generate a single value appropriate for `col`, name-pattern first.
    pub fn value_for(&self, col: &FakerColumn) -> CborValue {
        let name = col.name.to_lowercase();

        // --- name-aware ------------------------------------------------------
        if name.contains("email") {
            return CborValue::Text(SafeEmail().fake());
        }
        if name.contains("first") && name.contains("name") {
            return CborValue::Text(FirstName().fake());
        }
        if name.contains("last") && name.contains("name") || name.contains("surname") {
            return CborValue::Text(LastName().fake());
        }
        if name.contains("username") || name.contains("login") || name.contains("handle") {
            return CborValue::Text(Username().fake());
        }
        if name.contains("name") {
            return CborValue::Text(Name().fake());
        }
        if name.contains("phone") || name.contains("mobile") || name.contains("tel") {
            return CborValue::Text(PhoneNumber().fake());
        }
        if name.contains("city") {
            return CborValue::Text(CityName().fake());
        }
        if name.contains("country") {
            return CborValue::Text(CountryName().fake());
        }
        if name.contains("street") || name.contains("address") {
            return CborValue::Text(StreetName().fake());
        }
        if name.contains("company") || name.contains("employer") || name.contains("organization") {
            return CborValue::Text(CompanyName().fake());
        }

        // --- type fallback ---------------------------------------------------
        self.value_by_type(&col.ty)
    }

    fn value_by_type(&self, ty: &str) -> CborValue {
        match ty.trim().to_lowercase().as_str() {
            "int" | "integer" | "number" | "i64" | "bigint" => {
                let n: i64 = (0..10_000).fake();
                CborValue::Integer(n.into())
            }
            "decimal" | "float" | "double" | "money" | "amount" | "f64" => {
                // two-decimal money-like value without a float-formatting dep
                let cents: i64 = (0..1_000_000).fake();
                CborValue::Float(cents as f64 / 100.0)
            }
            "bool" | "boolean" => CborValue::Bool(fake::Faker.fake()),
            "datetime" | "date" | "timestamp" => {
                // avoid a wall-clock / chrono dependency — vary a plausible ISO string
                let day: u8 = (1..=28).fake();
                let hour: u8 = (0..24).fake();
                CborValue::Text(format!("2026-01-{day:02}T{hour:02}:00:00Z"))
            }
            // string and anything unknown
            _ => CborValue::Text(Word().fake()),
        }
    }

    /// Build a full record for `id`, filling every column. The id column is set
    /// to `id` verbatim; all others are generated.
    pub fn record_for(
        &self,
        columns: &[FakerColumn],
        id_column: &str,
        id: &str,
    ) -> Record<CborValue> {
        let mut rec = Record::new();
        let mut wrote_id = false;
        for col in columns {
            if col.name == id_column {
                rec.insert(col.name.clone(), CborValue::Text(id.to_string()));
                wrote_id = true;
            } else {
                rec.insert(col.name.clone(), self.value_for(col));
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
}
