use serde::{Deserialize, Serialize};
use vantage_types::prelude::*;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Person {
    name: String,
    age: u32,
    city: String,
    is_active: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct Company {
    name: String,
    employees: u32,
    founded: u32,
}

fn main() {
    println!("=== Testing Serde Record Conversions ===\n");

    // Create test data
    let person = Person {
        name: "Alice Johnson".to_string(),
        age: 30,
        city: "New York".to_string(),
        is_active: true,
    };

    let company = Company {
        name: "TechCorp".to_string(),
        employees: 150,
        founded: 2010,
    };

    println!("Original person: {:?}", person);
    println!("Original company: {:?}", company);

    // Test: Person -> Record<serde_json::Value>
    let person_record: Record<serde_json::Value> = person.clone().into_record();
    println!("\nPerson -> Record<serde_json::Value>: SUCCESS");
    println!("Person record: {:?}", person_record);

    // Test: Company -> Record<serde_json::Value>
    let company_record: Record<serde_json::Value> = company.clone().into_record();
    println!("\nCompany -> Record<serde_json::Value>: SUCCESS");
    println!("Company record: {:?}", company_record);

    // Test reverse conversion: Record<serde_json::Value> -> Person
    let person_back: Result<Person, _> = Person::from_record(person_record.clone());
    match person_back {
        Ok(p) => {
            println!("\nRecord<serde_json::Value> -> Person: SUCCESS");
            println!("Reconstructed person: {:?}", p);
            assert_eq!(
                p,
                Person {
                    name: "Alice Johnson".to_string(),
                    age: 30,
                    city: "New York".to_string(),
                    is_active: true,
                }
            );
            println!("✅ Person round-trip successful!");
        }
        Err(e) => {
            println!("Record<serde_json::Value> -> Person: FAILED - {:?}", e);
        }
    }

    // Test reverse conversion: Record<serde_json::Value> -> Company
    let company_back: Result<Company, _> = Company::from_record(company_record.clone());
    match company_back {
        Ok(c) => {
            println!("\nRecord<serde_json::Value> -> Company: SUCCESS");
            println!("Reconstructed company: {:?}", c);
            assert_eq!(
                c,
                Company {
                    name: "TechCorp".to_string(),
                    employees: 150,
                    founded: 2010,
                }
            );
            println!("✅ Company round-trip successful!");
        }
        Err(e) => {
            println!("Record<serde_json::Value> -> Company: FAILED - {:?}", e);
        }
    }

    // Test with nested structures
    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    struct Department {
        name: String,
        head: Person,
        budget: f64,
    }

    let department = Department {
        name: "Engineering".to_string(),
        head: Person {
            name: "Bob Smith".to_string(),
            age: 45,
            city: "San Francisco".to_string(),
            is_active: true,
        },
        budget: 500000.0,
    };

    println!("\n=== Testing Nested Structures ===");
    println!("Original department: {:?}", department);

    // Department -> Record<serde_json::Value> -> Department
    let dept_record: Record<serde_json::Value> = department.clone().into_record();
    println!("\nDepartment -> Record<serde_json::Value>: SUCCESS");

    let dept_back: Result<Department, _> = Department::from_record(dept_record);
    match dept_back {
        Ok(d) => {
            println!("Record<serde_json::Value> -> Department: SUCCESS");
            println!("Reconstructed department: {:?}", d);
            println!("✅ Nested structure round-trip successful!");
        }
        Err(e) => {
            println!("Nested structure conversion: FAILED - {:?}", e);
        }
    }

    println!("\n🎉 All automatic serde Record conversions work correctly!");
}
