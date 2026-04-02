use crate::Email;
use crate::Type2;
use serde_json::json;
use url::Url;

// Implementations for String
impl Type2 for String {
    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::String(self.clone())
    }

    fn from_json(json: &serde_json::Value) -> Option<Self> {
        json.as_str().map(|s| s.to_string())
    }
}

// Implementations for Url
impl Type2 for Url {
    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::String(self.to_string())
    }

    fn from_json(json: &serde_json::Value) -> Option<Self> {
        let s = json.as_str()?;
        Url::parse(s).ok()
    }
}

// Implementations for Email
impl Type2 for Email {
    fn to_json(&self) -> serde_json::Value {
        json!({
            "name": self.name,
            "domain": self.domain
        })
    }

    fn from_json(json: &serde_json::Value) -> Option<Self> {
        let obj = json.as_object()?;
        let name = obj.get("name")?.as_str()?;
        let domain = obj.get("domain")?.as_str()?;
        Some(Email::new(name, domain))
    }
}
