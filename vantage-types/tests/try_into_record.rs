//! `TryIntoRecord` is the fallible counterpart to `TryFromRecord`: serializing a
//! user entity into a `Record` can legitimately fail, so the conversion returns
//! `Result` instead of panicking deep inside a write path.
#![cfg(feature = "serde")]

use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;
use vantage_types::{Record, TryIntoRecord};

/// A field whose `Serialize` impl always errors, standing in for any entity
/// whose serialization can fail at runtime (non-string map keys, out-of-range
/// numbers, a hand-written `Serialize`, ...).
#[derive(Clone, Deserialize)]
struct Boom;

impl Serialize for Boom {
    fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom(
            "serialization deliberately failed",
        ))
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct HasBoom {
    boom: Boom,
}

#[test]
fn json_serialization_failure_is_an_error_not_a_panic() {
    let entity = HasBoom { boom: Boom };
    let result: Result<Record<serde_json::Value>, _> = entity.try_into_record();
    assert!(
        result.is_err(),
        "expected serialization error, got a record"
    );
}

#[test]
fn cbor_non_text_map_keys_are_an_error_not_silent_loss() {
    // A map keyed by integers serializes to a CBOR map with non-text keys.
    // The old `IntoRecord` impl dropped these silently; the fallible path
    // must surface them as an error.
    let mut entity: BTreeMap<i32, String> = BTreeMap::new();
    entity.insert(1, "a".to_string());
    entity.insert(2, "b".to_string());

    let result: Result<Record<ciborium::Value>, _> = entity.try_into_record();
    assert!(
        result.is_err(),
        "non-text CBOR keys must error rather than be dropped"
    );
}

#[test]
fn ok_for_a_plain_serde_struct() {
    #[derive(Clone, Serialize, Deserialize)]
    struct User {
        id: u32,
        name: String,
    }

    let record: Record<serde_json::Value> = User {
        id: 7,
        name: "Alice".to_string(),
    }
    .try_into_record()
    .expect("plain struct serializes cleanly");

    assert_eq!(record.get("name"), Some(&serde_json::json!("Alice")));
    assert_eq!(record.get("id"), Some(&serde_json::json!(7)));
}
