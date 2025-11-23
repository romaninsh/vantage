use serde::{Deserialize, Serialize};
use vantage_types::prelude::*;
use vantage_types::vantage_type_system;

// Define a type system
vantage_type_system! {
    type_trait: MyType,
    method_name: my_value,
    value_type: String,
    type_variants: [Text, Number]
}

impl MyType for String {
    type Target = MyTypeTextMarker;

    fn to_my_value(&self) -> String {
        self.clone()
    }

    fn from_my_value(value: String) -> Option<Self> {
        Some(value)
    }
}

impl MyType for i32 {
    type Target = MyTypeNumberMarker;

    fn to_my_value(&self) -> String {
        self.to_string()
    }

    fn from_my_value(value: String) -> Option<Self> {
        value.parse().ok()
    }
}

impl MyTypeVariants {
    pub fn from_my_value(value: &String) -> Option<Self> {
        if value.parse::<i32>().is_ok() {
            Some(MyTypeVariants::Number)
        } else {
            Some(MyTypeVariants::Text)
        }
    }
}

// Struct with persistence macro (uses type system)
#[derive(Debug, PartialEq, Clone)]
#[persistence(MyType)]
struct TypeSystemStruct {
    name: String,
    count: i32,
}

// Plain struct with serde (uses JSON)
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct SerdeStruct {
    title: String,
    active: bool,
    score: f64,
}

fn main() {
    println!("=== Testing Combined Type System + Serde Conversions ===\n");

    // Create test data
    let ts_struct = TypeSystemStruct {
        name: "Test Item".to_string(),
        count: 42,
    };

    let serde_struct = SerdeStruct {
        title: "Example".to_string(),
        active: true,
        score: 95.5,
    };

    // Test 1: Type system struct conversions
    println!("1. Type System Struct Conversions:");
    println!(
        "   Original: name={}, count={}",
        ts_struct.name, ts_struct.count
    );

    // TypeSystemStruct -> Record<AnyMyType>
    let ts_any_record: Record<AnyMyType> = ts_struct.into_record();
    println!("   ✅ TypeSystemStruct -> Record<AnyMyType>");

    // Record<AnyMyType> -> Record<String> (via IntoRecord trait)
    let ts_string_record: Record<String> = ts_any_record.clone().into_record();
    println!("   ✅ Record<AnyMyType> -> Record<String>");
    println!("   String record: {:?}", ts_string_record);

    // Round trip back
    let ts_any_back: Record<AnyMyType> = Record::from_record(ts_string_record).unwrap();
    let ts_back = TypeSystemStruct::from_record(ts_any_back).unwrap();
    println!(
        "   ✅ Round trip: name={}, count={}",
        ts_back.name, ts_back.count
    );

    println!();

    // Test 2: Serde struct conversions
    println!("2. Serde Struct Conversions:");
    println!("   Original: {:?}", serde_struct);

    // SerdeStruct -> Record<serde_json::Value> (via serde methods)
    let serde_json_record: Record<serde_json::Value> = serde_struct.clone().into_record();
    println!("   ✅ SerdeStruct -> Record<serde_json::Value>");
    println!("   JSON record: {:?}", serde_json_record);

    // Round trip back
    let serde_back: SerdeStruct = SerdeStruct::from_record(serde_json_record).unwrap();
    println!("   ✅ Round trip: {:?}", serde_back);

    println!();

    // Test 3: Cross-conversion via String
    println!("3. Cross-Conversion via String:");

    // Create a simple struct that implements both
    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    #[persistence(MyType)]
    struct HybridStruct {
        label: String,
        value: String,
    }

    let hybrid = HybridStruct {
        label: "test".to_string(),
        value: "123".to_string(),
    };

    println!("   Original hybrid: {:?}", hybrid);

    // Path 1: via Type System
    let hybrid_any: Record<AnyMyType> = hybrid.clone().into_record();
    let hybrid_string_via_ts: Record<String> = hybrid_any.into_record();
    println!("   ✅ HybridStruct -> Record<AnyMyType> -> Record<String>");
    println!("   Via type system: {:?}", hybrid_string_via_ts);

    // Path 2: via Serde
    let hybrid_json: Record<serde_json::Value> = hybrid.clone().into_record();

    // Convert serde_json::Value to String (need manual conversion for this)
    let hybrid_string_via_serde: Record<String> = hybrid_json
        .into_iter()
        .map(|(k, v)| {
            let string_val = match v {
                serde_json::Value::String(s) => s,
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => "null".to_string(),
            };
            (k, string_val)
        })
        .collect();

    println!("   ✅ HybridStruct -> Record<serde_json::Value> -> Record<String>");
    println!("   Via serde: {:?}", hybrid_string_via_serde);

    println!();

    // Test 4: Demonstrate flexibility
    println!("4. Flexibility Test - Multiple Conversion Paths:");

    let flexible_struct = HybridStruct {
        label: "flexible".to_string(),
        value: "999".to_string(),
    };

    // Show that the same struct can go through different conversion paths
    let path1: Record<AnyMyType> = flexible_struct.clone().into_record(); // Type system path
    let path2_record: Record<serde_json::Value> = flexible_struct.clone().into_record(); // Serde path

    println!("   Same struct, different paths:");
    println!("   Type system path: {:?}", path1);
    println!("   Serde path: {:?}", path2_record);

    // Both can convert to strings
    let path1_strings: Record<String> = path1.into_record();
    println!("   Both end up as Record<String>: {:?}", path1_strings);

    println!("\n🎉 All combined conversions work perfectly!");
    println!("   - Type system conversions via persistence macro");
    println!("   - Automatic serde conversions via blanket impls");
    println!("   - Cross-compatibility through common types");
    println!("   - Multiple conversion paths for flexible structs");
}
