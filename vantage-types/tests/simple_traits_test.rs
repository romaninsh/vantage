use vantage_types::{vantage_type_system, IntoRecord, Record, TryFromRecord};

vantage_type_system! {
    type_trait: TestType,
    method_name: test_value,
    value_type: String,
    type_variants: [Text, Label]
}

impl TestType for String {
    type Target = TestTypeTextMarker;

    fn to_test_value(&self) -> String {
        self.clone()
    }

    fn from_test_value(value: String) -> Option<Self> {
        Some(value)
    }
}

struct Label(String);

impl TestType for Label {
    type Target = TestTypeLabelMarker;

    fn to_test_value(&self) -> String {
        format!("label:{}", self.0)
    }

    fn from_test_value(value: String) -> Option<Self> {
        if value.starts_with("label:") {
            Some(Label(value[6..].to_string()))
        } else {
            None
        }
    }
}

impl TestTypeVariants {
    pub fn from_test_value(value: &String) -> Option<Self> {
        if value.starts_with("label:") {
            Some(TestTypeVariants::Label)
        } else {
            Some(TestTypeVariants::Text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_individual_conversions() {
        // Test String conversions
        let text = String::from("hello");
        let any_type = AnyTestType::new(text.clone());

        // AnyType -> ValueType
        let value: String = any_type.clone().into();
        assert_eq!(value, "hello");

        // ValueType -> AnyType
        let any_type2: AnyTestType = "world".to_string().try_into().unwrap();
        assert_eq!(any_type2.value(), "world");
        assert_eq!(any_type2.type_variant(), Some(TestTypeVariants::Text));
    }

    #[test]
    fn test_record_conversions() {
        // Create Record<AnyTestType>
        let mut any_record: Record<AnyTestType> = Record::new();
        any_record.insert("name".to_string(), AnyTestType::new("Alice".to_string()));
        any_record.insert(
            "label".to_string(),
            AnyTestType::new(Label("important".to_string())),
        );

        // Record<AnyTestType> -> Record<String>
        let string_record: Record<String> = any_record.clone().into_record();
        assert_eq!(string_record.get("name").unwrap(), "Alice");
        assert_eq!(string_record.get("label").unwrap(), "label:important");

        // Record<String> -> Record<AnyTestType>
        let back_to_any: Record<AnyTestType> = Record::from_record(string_record).unwrap();

        let name_any = back_to_any.get("name").unwrap();
        assert_eq!(name_any.value(), "Alice");
        assert_eq!(name_any.type_variant(), Some(TestTypeVariants::Text));

        let label_any = back_to_any.get("label").unwrap();
        assert_eq!(label_any.value(), "label:important");
        assert_eq!(label_any.type_variant(), Some(TestTypeVariants::Label));
    }

    #[test]
    fn test_record_conversion_error_handling() {
        // Create a Record<String> that can't convert back to AnyTestType
        let mut string_record: Record<String> = Record::new();
        string_record.insert("valid".to_string(), "hello".to_string());
        string_record.insert("invalid".to_string(), "".to_string()); // Empty string might fail conversion

        // This should still work since our implementations are permissive
        let result: Result<Record<AnyTestType>, _> = Record::from_record(string_record);
        assert!(result.is_ok());
    }
}
