// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! SurrealDB client speaking CBOR over WebSocket (`ws://`, `wss://`, or
//! `cbor://` as an alias).

mod cbor_convert;
pub mod client;
pub mod connection;
pub mod engine;
pub mod engines;
pub mod error;
pub mod live;
pub mod mocks;
pub mod params;
#[cfg(feature = "pool")]
pub mod pool;
pub mod record;
pub mod session;

pub use client::SurrealClient;
pub use connection::SurrealConnection;

pub use engine::Engine;
pub use engines::{DebugEngine, WsCborEngine};
pub use error::{Result, SurrealError};
pub use live::{Action, LiveStream, Notification};
pub use mocks::{MockSurrealEngine, SurrealMockBuilder};
pub use record::{
    RecordId, RecordIdValue, RecordRange, Table, escape_identifier, escape_record_id,
};
pub use session::SessionState;
