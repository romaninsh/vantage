//! Decimal type implementations for SurrealType trait using vantage-types

use super::{SurrealType, SurrealTypeDecimalMarker};
use ciborium::value::Value as CborValue;

impl SurrealType for rust_decimal::Decimal {
    type Target = SurrealTypeDecimalMarker;

    fn to_cbor(&self) -> CborValue {
        // Store decimal as CBOR tag 200 with string representation to avoid precision loss
        CborValue::Tag(200, Box::new(CborValue::Text(self.to_string())))
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Tag(200, boxed_value) => {
                if let CborValue::Text(decimal_str) = boxed_value.as_ref() {
                    decimal_str.parse().ok()
                } else {
                    None
                }
            }
            CborValue::Text(s) => {
                // Allow direct string parsing as fallback
                s.parse().ok()
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal as RustDecimal;

    #[test]
    fn test_rust_decimal() {
        let dec = RustDecimal::new(12345, 2); // 123.45
        let cbor = dec.to_cbor();
        let restored = RustDecimal::from_cbor(cbor).unwrap();
        assert_eq!(dec, restored);
    }

    #[test]
    fn test_large_decimal() {
        let large_decimal = "999999999999999999.999999999999999999"
            .parse::<RustDecimal>()
            .unwrap();

        let cbor = large_decimal.to_cbor();
        let restored = RustDecimal::from_cbor(cbor).unwrap();
        assert_eq!(large_decimal, restored);
    }

    #[test]
    fn test_decimal_precision() {
        let precise = "0.000000000000000001".parse::<RustDecimal>().unwrap();
        let cbor = precise.to_cbor();
        let restored = RustDecimal::from_cbor(cbor).unwrap();
        assert_eq!(precise, restored);
    }
}
