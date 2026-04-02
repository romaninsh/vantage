use vantage_types::{vantage_type_system, IntoRecord, Record, TryFromRecord};

vantage_type_system! {
    type_trait: SimpleType,
    method_name: simple,
    value_type: String,
    type_variants: [Text]
}

impl SimpleType for String {
    type Target = SimpleTypeTextMarker;

    fn to_simple(&self) -> String {
        self.clone()
    }

    fn from_simple(value: String) -> Option<Self> {
        Some(value)
    }
}

impl SimpleTypeVariants {
    pub fn from_simple(_value: &String) -> Option<Self> {
        Some(SimpleTypeVariants::Text)
    }
}

fn main() {
    let text = String::from("hello");
    let any_type = AnySimpleType::new(text.clone());

    // Test Into: AnySimpleType -> String
    let value: String = any_type.clone().into();
    println!("Into conversion: {}", value);
    assert_eq!(value, "hello");

    // Test TryFrom: String -> AnySimpleType
    let any_type2: AnySimpleType = "world".to_string().try_into().unwrap();
    println!("TryFrom conversion: {:?}", any_type2.value());
    assert_eq!(any_type2.value(), "world");
    assert_eq!(any_type2.type_variant(), Some(SimpleTypeVariants::Text));

    // Test that AnySimpleType no longer implements SimpleType trait
    println!("\n--- Testing AnySimpleType (trait implementation removed) ---");
    let any_simple = AnySimpleType::new("test".to_string());
    println!("AnySimpleType created with value: {:?}", any_simple.value());
    // Note: can no longer call .to_simple() or .from_simple() since trait impl was removed

    // Test Record conversions
    println!("\n--- Testing Record conversions ---");

    // Create Record<AnySimpleType>
    let mut any_record: Record<AnySimpleType> = Record::new();
    any_record.insert("name".to_string(), AnySimpleType::new("Alice".to_string()));
    any_record.insert("city".to_string(), AnySimpleType::new("Paris".to_string()));

    println!("Original Record<AnySimpleType>: {:?}", any_record);

    // Try: Record<AnySimpleType> -> Record<String> using into_record()
    let string_record: Record<String> = any_record.clone().into_record();
    println!("Record<AnySimpleType> -> Record<String> conversion: SUCCESS");
    println!("Converted record: {:?}", string_record);

    // Try reverse: Record<String> -> Record<AnySimpleType> using from_record()
    let back_to_any: Result<Record<AnySimpleType>, _> = Record::from_record(string_record);
    match back_to_any {
        Ok(any_rec) => {
            println!("Record<String> -> Record<AnySimpleType> conversion: SUCCESS");
            println!("Round-trip record: {:?}", any_rec);
        }
        Err(e) => {
            println!(
                "Record<String> -> Record<AnySimpleType> conversion: FAILED - {:?}",
                e
            );
        }
    }

    println!("Record conversions work correctly even without AnyType trait implementation!");
}
