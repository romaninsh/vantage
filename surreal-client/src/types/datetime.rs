//! DateTime and Duration type implementations for SurrealType trait using vantage-types
//! Uses standard Rust types directly without wrappers

// Standard chrono and std types use SurrealType implementations from standard.rs

// Standard chrono::DateTime<Utc> already has SurrealType implementation in standard.rs
// Standard std::time::Duration already has SurrealType implementation in standard.rs
// Standard std::time::SystemTime already has SurrealType implementation in standard.rs

// Re-export for convenience
pub use chrono::{DateTime, Utc};
pub use std::time::{Duration, SystemTime};

#[cfg(test)]
mod tests {
    use crate::types::SurrealType;

    #[test]
    fn test_chrono_datetime() {
        let dt = chrono::Utc::now();
        let cbor = dt.to_cbor();
        let restored = chrono::DateTime::<chrono::Utc>::from_cbor(cbor).unwrap();

        // Should be very close (within a second due to precision)
        let diff = (dt.timestamp() - restored.timestamp()).abs();
        assert!(diff <= 1);
    }

    #[test]
    fn test_system_time() {
        let now = std::time::SystemTime::now();
        let cbor = now.to_cbor();
        let restored = std::time::SystemTime::from_cbor(cbor).unwrap();

        // Should round-trip successfully
        let duration_diff = now
            .duration_since(restored)
            .unwrap_or(restored.duration_since(now).unwrap());
        assert!(duration_diff.as_secs() <= 1);
    }

    #[test]
    fn test_duration() {
        let dur = std::time::Duration::new(42, 123_456_789);
        let cbor = dur.to_cbor();
        let restored = std::time::Duration::from_cbor(cbor).unwrap();

        assert_eq!(dur, restored);
    }

    #[test]
    fn test_chrono_duration() {
        let dur = chrono::Duration::seconds(42);
        let cbor = dur.to_cbor();
        let restored = chrono::Duration::from_cbor(cbor).unwrap();

        // Should be equal within reasonable precision
        let diff = (dur.num_seconds() - restored.num_seconds()).abs();
        assert!(diff <= 1);
    }
}
