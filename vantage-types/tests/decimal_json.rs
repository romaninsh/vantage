use rust_decimal::Decimal;
use serde_json::Value;
use vantage_types::{persistence, vantage_type_system};

// Generate MyType system using JSON value type
vantage_type_system! {
    type_trait: MyType,
    method_name: json,
    value_type: serde_json::Value,
    type_variants: [Int, Float, Decimal, String]
}

// Override the macro-generated variant detection with our custom logic
impl MyTypeVariants {
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        match value {
            Value::Number(n) if n.is_i64() => Some(MyTypeVariants::Int),
            Value::Number(n) if n.is_f64() => Some(MyTypeVariants::Float),
            Value::String(_) => Some(MyTypeVariants::String),
            Value::Object(obj) => {
                if obj.contains_key("decimal") {
                    Some(MyTypeVariants::Decimal)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl MyType for i64 {
    type Target = MyTypeIntMarker;

    fn to_json(&self) -> serde_json::Value {
        Value::Number(serde_json::Number::from(*self))
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64(),
            _ => None,
        }
    }
}

impl MyType for f64 {
    type Target = MyTypeFloatMarker;

    fn to_json(&self) -> serde_json::Value {
        serde_json::Number::from_f64(*self)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_f64(),
            _ => None,
        }
    }
}

impl MyType for Decimal {
    type Target = MyTypeDecimalMarker;

    fn to_json(&self) -> serde_json::Value {
        // Store decimal as {"decimal": "decimal_string"} to avoid precision loss
        serde_json::json!({"decimal": self.to_string()})
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::Object(obj) => {
                if let Some(Value::String(decimal_str)) = obj.get("decimal") {
                    decimal_str.parse().ok()
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl MyType for String {
    type Target = MyTypeStringMarker;

    fn to_json(&self) -> serde_json::Value {
        Value::String(self.clone())
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decimal_conversion() {
        let decimal = Decimal::from_str_exact("123123123.123123123").unwrap();
        let any_decimal = AnyMyType::new(decimal);

        // Verify it's stored with correct type
        assert_eq!(any_decimal.type_variant(), Some(MyTypeVariants::Decimal));

        // Verify the JSON value is in correct format
        let json_value = any_decimal.value();
        let expected = serde_json::json!({"decimal": "123123123.123123123"});
        assert_eq!(json_value, &expected);

        // Test round-trip conversion
        let restored_any = AnyMyType::from_json(json_value).unwrap();
        let restored_decimal: Decimal = restored_any.try_get().unwrap();
        assert_eq!(decimal, restored_decimal);

        // Attempt to restore decimal as string - confirm it fails
        let failed_string: Option<String> = restored_any.try_get();
        assert!(failed_string.is_none());

        // Attempt to restore decimal as number - confirm it fails
        let failed_number: Option<i64> = restored_any.try_get();
        assert!(failed_number.is_none());
    }

    #[test]
    fn test_record_persistence() {
        #[derive(Debug, PartialEq)]
        #[persistence(MyType)]
        struct Record {
            amount: Decimal,
        }

        let record = Record {
            amount: Decimal::from_str_exact("123123123.123123123").unwrap(),
        };

        // Store record
        let storage_map = record.to_mytype_map();

        // Verify stored value format
        let amount_json = storage_map.get("amount").unwrap().value();
        assert_eq!(
            amount_json,
            &serde_json::json!({"decimal": "123123123.123123123"})
        );

        // Restore record back
        let restored = Record::from_mytype_map(storage_map.clone()).unwrap();
        assert_eq!(record, restored);

        // Attempt to convert into record where amount is String - confirm it fails
        #[derive(Debug)]
        #[persistence(MyType)]
        struct StringRecord {
            amount: String,
        }

        // Attempt to restore as StringRecord should fail
        let failed_restore = StringRecord::from_mytype_map(storage_map);
        assert!(failed_restore.is_none());
    }

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
