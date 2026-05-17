/// Internal bus message published by a Dio for its Sceneries (and any
/// curious user callbacks) to consume.
///
/// Distinct from [`crate::ops::ChangeEvent`] — `ChangeEvent` is the
/// *upstream* shape (what a SurrealDB LIVE stream or a webhook delivers
/// about the master backend), while `DioEvent` is the *internal* fanout
/// shape Sceneries react to.
#[derive(Debug, Clone)]
pub enum DioEvent {
    RecordChanged { id: String },
    RecordInserted { id: String },
    RecordRemoved { id: String },
    Invalidated,
    Refreshing,
    WriteFailed { id: Option<String>, error: String },
}
