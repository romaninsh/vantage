use ciborium::Value as CborValue;
use vantage_types::Record;

/// Upstream change observation handed to an `on_event` callback.
///
/// This is the *external* event vocabulary — what a SurrealDB LIVE stream,
/// a Kafka topic, or a SaaS webhook delivers about the master backend.
/// Distinct from [`crate::dio::DioEvent`], which is the *internal* bus
/// the Dio publishes onto for Sceneries to consume.
#[derive(Debug, Clone)]
pub enum ChangeEvent {
    Updated {
        id: String,
        new: Record<CborValue>,
    },
    Inserted {
        id: String,
    },
    Deleted {
        id: String,
    },
    Invalidated,
}
