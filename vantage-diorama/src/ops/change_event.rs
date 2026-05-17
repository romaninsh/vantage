use ciborium::Value as CborValue;
use vantage_types::Record;

/// Upstream change observation handed to an `on_event` callback.
///
/// This is the *external* event vocabulary — what a SurrealDB LIVE
/// stream, a Kafka topic, or a SaaS webhook delivers about the master
/// backend. Distinct from [`crate::dio::DioEvent`], which is the
/// *internal* bus the Dio publishes onto for Sceneries to consume.
///
/// `new` is optional on `Updated` / `Inserted` — the source decides
/// whether to ship the full record. SurrealDB LIVE provides values;
/// a polling diff might only know the id and leave the callback to
/// refetch via `dio.master().get_value(&id)`.
#[derive(Debug, Clone)]
pub enum ChangeEvent {
    Updated {
        id: String,
        new: Option<Record<CborValue>>,
    },
    Inserted {
        id: String,
        new: Option<Record<CborValue>>,
    },
    Deleted {
        id: String,
    },
    Invalidated,
}
