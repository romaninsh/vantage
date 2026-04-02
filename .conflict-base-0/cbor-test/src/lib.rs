use serde::{Deserialize, Serialize};
use url::Url;

pub mod cbor;
pub mod generic;
pub mod type1;
pub mod type2;

use generic::field_type_system;

field_type_system! {
    type_trait: Type1,
    method_name: cbor,
    value_type: ciborium::value::Value,
}

field_type_system! {
    type_trait: Type2,
    method_name: json,
    value_type: serde_json::Value,
}

// Custom Email struct
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Email {
    pub name: String,
    pub domain: String,
}

impl Email {
    pub fn new(name: &str, domain: &str) -> Self {
        Self {
            name: name.to_string(),
            domain: domain.to_string(),
        }
    }
}

// Record struct containing all three field types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    pub website: Url,
    pub email: Email,
}

impl Record {
    pub fn new(name: String, website: Url, email: Email) -> Self {
        Self {
            name,
            website,
            email,
        }
    }
}
