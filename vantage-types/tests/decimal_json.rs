use rust_decimal::Decimal;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_loss_without_vantage_system() {
        let original_decimal = Decimal::from_str_exact("123123123.123123123").unwrap();

        // 1. Serialize directly - all good
        let json_string = serde_json::to_string(&original_decimal).unwrap();

        // 2. Deserialize directly - all good
        let back_to_decimal: Decimal = serde_json::from_str(&json_string).unwrap();
        assert_eq!(original_decimal, back_to_decimal);

        // 3. Deserialize into string - whoops, shouldn't work but it does!
        let as_string: String = serde_json::from_str(&json_string).unwrap();
        // This succeeds but loses all type semantics - now it's just text!

        // This demonstrates the problem: JSON consumers can't tell if "123123123.123123123"
        // was meant to be a Decimal, String, or something else
        assert_eq!(as_string, "123123123.123123123");
    }

    #[test]
    fn test_record_type_confusion() {
        use serde::{Deserialize, Serialize};

        // Define two similar records with different field types
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct IntRecord {
            amount: i64,
        }

        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct DecimalRecord {
            amount: Decimal,
        }

        // Original record with i64
        let original = IntRecord { amount: 100 };

        // Store into JSON
        let json_string = serde_json::to_string(&original).unwrap();

        // Load into other record type (this works! but shouldn't)
        let mut as_decimal_rec: DecimalRecord = serde_json::from_str(&json_string).unwrap();

        // Increase by one
        as_decimal_rec.amount += Decimal::from(1);

        // Store back to JSON
        let modified_json = serde_json::to_string(&as_decimal_rec).unwrap();

        // Attempt to load into original struct - this fails!
        let failed_load: Result<IntRecord, _> = serde_json::from_str(&modified_json);
        assert!(failed_load.is_err());

        // This demonstrates how type confusion can break data integrity
        // The decimal serializes differently than the integer
    }
}
