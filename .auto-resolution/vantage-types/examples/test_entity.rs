use vantage_types::{vantage_type_system, IntoRecord, Record, TryFromRecord};
use vantage_types_entity::entity;

vantage_type_system! {
    type_trait: TestType,
    method_name: test_value,
    value_type: String,
    type_variants: [Text]
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

impl TestTypeVariants {
    pub fn from_test_value(_value: &String) -> Option<Self> {
        Some(TestTypeVariants::Text)
    }
}

#[entity(TestType)]
struct MyStruct {
    name: String,
    city: String,
}

fn main() {
    // Create a struct
    let my_struct = MyStruct {
        name: "Alice".to_string(),
        city: "Paris".to_string(),
    };

    println!(
        "Original struct: name={}, city={}",
        my_struct.name, my_struct.city
    );

    // Test: MyStruct -> Record<AnyTestType> using into_record()
    let any_record: Record<AnyTestType> = my_struct.into_record();
    println!("MyStruct -> Record<AnyTestType>: SUCCESS");
    println!("Record<AnyTestType>: {:?}", any_record);

    // Test: Record<AnyTestType> -> Record<String> using blanket implementation
    let string_record: Record<String> = any_record.clone().into_record();
    println!("Record<AnyTestType> -> Record<String>: SUCCESS");
    println!("Record<String>: {:?}", string_record);

    // Test reverse: Record<AnyTestType> -> MyStruct using from_record()
    let back_to_struct: MyStruct = MyStruct::from_record(any_record).unwrap();
    println!("Record<AnyTestType> -> MyStruct: SUCCESS");
    println!(
        "Reconstructed struct: name={}, city={}",
        back_to_struct.name, back_to_struct.city
    );

    // Test full round trip: MyStruct -> Record<String> -> MyStruct
    let original = MyStruct {
        name: "Bob".to_string(),
        city: "London".to_string(),
    };

    // MyStruct -> Record<AnyTestType> -> Record<String>
    let any_record: Record<AnyTestType> = original.into_record();
    let string_via_any: Record<String> = any_record.into_record();

    // Record<String> -> Record<AnyTestType> -> MyStruct
    let any_record_back: Record<AnyTestType> = Record::from_record(string_via_any).unwrap();
    let round_trip_result: Result<MyStruct, _> = MyStruct::from_record(any_record_back);

    match round_trip_result {
        Ok(reconstructed) => {
            println!("Full round trip: SUCCESS");
            println!(
                "Final result: name={}, city={}",
                reconstructed.name, reconstructed.city
            );
        }
        Err(e) => {
            println!("Full round trip: FAILED - {:?}", e);
        }
    }

    println!("All entity conversions work correctly!");
}
